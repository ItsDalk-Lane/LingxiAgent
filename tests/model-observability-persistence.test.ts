/**
 * Phase 7 Persistence Coordinator 测试（任务书 §一百零七～一百一十四/四十三/八十四/八十五）：
 * batch flush 无 duplicate / write failure 不影响业务 + 计数 / queue overflow
 * 不 block 不 throw + 显式计数 + dropped 标记 / Trace 优先 / graceful flush /
 * uninstall 恢复先前注册对象 / composite 转发 / retention（payload 先过期、
 * trace 整树删除、Usage Ledger 不受影响）/ drop 计数跨 restart 恢复。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { installModelObservabilityPersistence } from "../lib/llm/model-observability-persistence.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";
import { setModelCallObserver, getModelCallObserver } from "../lib/llm/model-call-observer.ts";
import {
  getModelCallBlobExternalizer,
  getModelCallPayloadSink,
  setModelCallBlobExternalizer,
  setModelCallPayloadSink,
} from "../lib/llm/model-call-payload-capture.ts";
import { createTestModelCallPayloadSink } from "../lib/llm/model-call-payload-testing.ts";
import { loadBetterSqliteDatabase } from "../lib/llm/model-observability-schema.ts";

const MODEL = { provider: "openai", modelId: "gpt-test", api: "openai-completions" };
const SOURCE = { subsystem: "test", operation: "unit", surface: "server", trigger: "test" };

function lifecycleEvent(callId: string, traceId: string, n: number) {
  return {
    eventType: "logical_call_start" as const,
    timestamp: new Date(Date.now() + n).toISOString(),
    callId,
    attemptId: null,
    traceId,
    parentCallId: null,
    model: MODEL,
    source: SOURCE,
    attribution: { kind: "test", sessionId: `s-${callId}` },
  };
}

function payloadRecord(callId: string, traceId: string, n: number) {
  return {
    schemaVersion: 1 as const,
    kind: "semantic_request" as const,
    capturedAt: new Date(Date.now() + n).toISOString(),
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
    payload: { inputShape: "calltext", systemPrompt: `prompt ${n}` },
  };
}

describe("Model Observability Persistence Coordinator", () => {
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;

  beforeEach(() => {
    harness = createModelObservabilityTestHarness();
  });
  afterEach(async () => {
    await harness.close();
    harness.cleanup();
    setModelCallObserver(null);
    setModelCallPayloadSink(null);
    setModelCallBlobExternalizer(null);
  });

  it("批量 flush：100 events + 100 payloads 无 duplicate、order 可恢复（§一百零七）", () => {
    for (let i = 0; i < 100; i += 1) {
      harness.handle.observer!.handleModelCallEvent(lifecycleEvent(`mc_batch_${i}`, "mt_batch", i));
      harness.handle.sink!.handleModelCallPayloadRecord(payloadRecord(`mc_batch_${i}`, "mt_batch", i));
    }
    harness.flush();
    const reader = harness.openReader();
    try {
      expect(reader.traceStore.getTrace("mt_batch")!.call_count).toBe(100);
      const calls = reader.db.prepare(`SELECT COUNT(*) AS n FROM model_calls WHERE trace_id = 'mt_batch'`).get();
      expect(calls.n).toBe(100);
      const payloads = reader.db.prepare(`SELECT COUNT(*) AS n FROM payload_records`).get();
      expect(payloads.n).toBe(100);
      // 每 call 恰一条 payload（无 duplicate）。
      const dup = reader.db.prepare(
        `SELECT call_id, COUNT(*) AS n FROM payload_records GROUP BY call_id HAVING n > 1`,
      ).all();
      expect(dup).toEqual([]);
    } finally {
      reader.close();
    }
  });

  it("write failure：flush transaction 失败 → 业务 handler 不 throw、writeFailures 计数、batch 被诚实 drop（§一百零八/四十九）", async () => {
    await harness.close(); // 换 failing-transaction DB 的独立 home。
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-wf-"));
    try {
      const RealDatabase = loadBetterSqliteDatabase();
      let txnCount = 0;
      class FailingAfterFirstTransaction {
        _real: any;
        constructor(...args: any[]) {
          this._real = new RealDatabase(...args);
        }
        pragma(...args: any[]) { return this._real.pragma(...args); }
        prepare(...args: any[]) { return this._real.prepare(...args); }
        exec(...args: any[]) { return this._real.exec(...args); }
        close() { return this._real.close(); }
        transaction(fn: any) {
          txnCount += 1;
          if (txnCount > 1) {
            return () => { throw new Error("simulated flush write failure"); };
          }
          return this._real.transaction(fn);
        }
      }
      const handle = installModelObservabilityPersistence({
        lingxiHome: home,
        policy: { enabled: true, persistPayloads: true },
        Database: FailingAfterFirstTransaction as any,
      });
      expect(handle.getHealth().status).toBe("active");
      // 模型热路径 handler：enqueue 永不 throw（业务不受影响）。
      expect(() => {
        handle.observer!.handleModelCallEvent(lifecycleEvent("mc_wf", "mt_wf", 0));
        handle.sink!.handleModelCallPayloadRecord(payloadRecord("mc_wf", "mt_wf", 0));
      }).not.toThrow();
      handle.flushSync();
      const health = handle.getHealth();
      expect(health.writeFailures).toBe(1);
      // 重试再失败 → 整批 drop 显式计数（§四十九：rollback 后才 retry；再失败诚实丢）。
      expect(health.droppedTraceEvents).toBe(1);
      expect(health.droppedPayloadRecords).toBe(1);
      await handle.close();
    } finally {
      fs.rmSync(home, { recursive: true, force: true });
    }
  });

  it("queue overflow：容量 2 → 不 block 不 throw、drop 计数、call 标记 dropped（§一百零九）", async () => {
    await harness.close();
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-qo-"));
    try {
      const handle = installModelObservabilityPersistence({
        lingxiHome: home,
        policy: {
          enabled: true,
          persistPayloads: true,
          // trace 容量足够大：dropped payload 的 call row 存在，dropped 标记才能落库。
          limits: { maxQueuedTraceEvents: 64, maxQueuedPayloadRecords: 2 },
        },
      });
      for (let i = 0; i < 5; i += 1) {
        expect(() => {
          handle.observer!.handleModelCallEvent(lifecycleEvent(`mc_q_${i}`, "mt_q", i));
          handle.sink!.handleModelCallPayloadRecord(payloadRecord(`mc_q_${i}`, "mt_q", i));
        }).not.toThrow();
      }
      const health = handle.getHealth();
      expect(health.droppedTraceEvents).toBe(0); // trace 容量独立（§一百一十）
      expect(health.droppedPayloadRecords).toBe(3);
      // 再来一条 trace event 触发 flush：dropped call 标记落库。
      handle.observer!.handleModelCallEvent(lifecycleEvent("mc_q_trigger", "mt_q", 99));
      handle.flushSync();
      const schema = await import("../lib/llm/model-observability-schema.ts");
      const db = schema.openModelObservabilityDatabase(schema.modelObservabilityDbPath(home));
      try {
        const marked = db.prepare(
          `SELECT call_id FROM model_calls WHERE payload_availability = 'dropped'`,
        ).all().map((r: any) => r.call_id);
        expect(marked.length).toBeGreaterThan(0);
      } finally {
        db.close();
      }
      await handle.close();
    } finally {
      fs.rmSync(home, { recursive: true, force: true });
    }
  });

  it("Trace 优先：payload queue 已满时 trace event 仍有自己的容量（§一百一十/四十一）", async () => {
    await harness.close();
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-tp-"));
    try {
      const handle = installModelObservabilityPersistence({
        lingxiHome: home,
        policy: {
          enabled: true,
          persistPayloads: true,
          limits: { maxQueuedPayloadRecords: 1, maxQueuedTraceEvents: 64 },
        },
      });
      handle.sink!.handleModelCallPayloadRecord(payloadRecord("mc_big_response", "mt_tp", 0));
      // 1MB 级 response 挤满 payload queue 之后……
      handle.sink!.handleModelCallPayloadRecord({
        ...payloadRecord("mc_big_response_2", "mt_tp", 1),
        payload: { inputShape: "calltext", systemPrompt: "x".repeat(1_000_000) },
      });
      expect(handle.getHealth().droppedPayloadRecords).toBe(1);
      // ……trace metadata 仍完整入队并最终落盘。
      handle.observer!.handleModelCallEvent(lifecycleEvent("mc_trace_survivor", "mt_tp", 2));
      handle.flushSync();
      const schema = await import("../lib/llm/model-observability-schema.ts");
      const db = schema.openModelObservabilityDatabase(schema.modelObservabilityDbPath(home));
      try {
        const call = db.prepare(`SELECT call_id FROM model_calls WHERE call_id = 'mc_trace_survivor'`).get();
        expect(call).toBeDefined();
        const trace = db.prepare(`SELECT call_count FROM traces WHERE trace_id = 'mt_tp'`).get();
        expect(trace.call_count).toBeGreaterThanOrEqual(1);
      } finally {
        db.close();
      }
      await handle.close();
    } finally {
      fs.rmSync(home, { recursive: true, force: true });
    }
  });

  it("graceful flush：close() 前队列非空 → 关闭完成后记录真正 durable（§一百一十一）", async () => {
    const rec = createModelCallRecorder({
      observer: harness.handle.observer,
      context: {
        callId: "mc_graceful",
        traceId: "mt_graceful",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    rec.beginLogicalCall({});
    rec.beginAttempt();
    rec.endLogicalCall("ok");
    harness.handle.sink!.handleModelCallPayloadRecord(payloadRecord("mc_graceful", "mt_graceful", 0));
    await harness.close(); // close 内部 flush
    const reader = harness.openReader();
    try {
      const call = reader.traceStore.getCall("mc_graceful");
      expect(call).not.toBeNull();
      expect(call.terminal_status).toBe("ok");
      expect(reader.payloadStore.getPayloadRecords("mc_graceful")).toHaveLength(1);
    } finally {
      reader.close();
    }
  });

  it("composite：安装期间既有 test observer/sink 仍收到事件（§八十四）；close 后恢复原注册对象（§八十五）", async () => {
    await harness.close();
    const priorObserver = createTestModelCallObserver();
    const priorSink = createTestModelCallPayloadSink();
    setModelCallObserver(priorObserver);
    setModelCallPayloadSink(priorSink);

    const handle = installModelObservabilityPersistence({
      lingxiHome: harness.lingxiHome,
      policy: { enabled: true, persistPayloads: true },
    });
    const observer = handle.observer!;
    observer.handleModelCallEvent(lifecycleEvent("mc_composite", "mt_comp", 0));
    handle.sink!.handleModelCallPayloadRecord(payloadRecord("mc_composite", "mt_comp", 0));
    expect(priorObserver.events.length).toBe(1);
    expect(priorSink.records.length).toBe(1);
    handle.flushSync();
    await handle.close();
    // 恢复先前注册对象（不是 NOOP 覆盖）。
    expect(getModelCallObserver()).toBe(priorObserver);
    expect(getModelCallPayloadSink()).toBe(priorSink);
  });

  it("Blob externalizer 只恢复安装前对象，不覆盖后来接管者", async () => {
    await harness.close();
    const priorExternalizer = { stageBinary: () => null };
    const successorExternalizer = { stageBinary: () => null };
    setModelCallBlobExternalizer(priorExternalizer);

    const first = installModelObservabilityPersistence({
      lingxiHome: harness.lingxiHome,
      policy: { enabled: true, persistPayloads: true, persistBlobs: true },
    });
    expect(getModelCallBlobExternalizer()).not.toBe(priorExternalizer);
    await first.close();
    expect(getModelCallBlobExternalizer()).toBe(priorExternalizer);

    const second = installModelObservabilityPersistence({
      lingxiHome: harness.lingxiHome,
      policy: { enabled: true, persistPayloads: true, persistBlobs: true },
    });
    setModelCallBlobExternalizer(successorExternalizer);
    await second.close();
    expect(getModelCallBlobExternalizer()).toBe(successorExternalizer);
  });

  it("drop 计数跨 restart 恢复（§四十三）", async () => {
    await harness.close();
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-cnt-"));
    try {
      const first = installModelObservabilityPersistence({
        lingxiHome: home,
        policy: { enabled: true, persistPayloads: true, limits: { maxQueuedPayloadRecords: 1 } },
      });
      first.sink!.handleModelCallPayloadRecord(payloadRecord("mc_c1", "mt_c", 0));
      first.sink!.handleModelCallPayloadRecord(payloadRecord("mc_c2", "mt_c", 1));
      first.sink!.handleModelCallPayloadRecord(payloadRecord("mc_c3", "mt_c", 2));
      expect(first.getHealth().droppedPayloadRecords).toBe(2);
      await first.close();
      const second = installModelObservabilityPersistence({
        lingxiHome: home,
        policy: { enabled: true, persistPayloads: true },
      });
      expect(second.getHealth().droppedPayloadRecords).toBe(2);
      await second.close();
    } finally {
      fs.rmSync(home, { recursive: true, force: true });
    }
  });

  it("retention：old trace payload 过期（call metadata 保留 + expired 标记），new trace 不动（§一百一十三/一百一十四）", () => {
    harness.handle.observer!.handleModelCallEvent(lifecycleEvent("mc_old", "mt_old", 0));
    harness.handle.observer!.handleModelCallEvent(lifecycleEvent("mc_new", "mt_new", 1));
    harness.handle.sink!.handleModelCallPayloadRecord(payloadRecord("mc_old", "mt_old", 0));
    harness.handle.sink!.handleModelCallPayloadRecord(payloadRecord("mc_new", "mt_new", 1));
    harness.flush();
    // 把 mt_old 拉回 40 天前（payload retention 30d < trace retention 180d）。
    const reader = harness.openReader();
    try {
      const old = new Date(Date.now() - 40 * 24 * 3600 * 1000).toISOString();
      reader.db.prepare(`UPDATE traces SET last_seen_at = ?, first_seen_at = ? WHERE trace_id = 'mt_old'`).run(old, old);
    } finally {
      reader.close();
    }
    // Usage Ledger 语义独立（§五十九）：预置 sentinel 验证 maintenance 不触碰它。
    const ledgerPath = path.join(harness.lingxiHome, "usage-ledger.json");
    fs.writeFileSync(ledgerPath, JSON.stringify({ sentinel: "untouched" }));

    const stats = harness.handle.runMaintenance();
    expect(stats).not.toBeNull();
    expect(stats!.payloadExpiredTraces).toBe(1);
    expect(stats!.payloadExpiredCalls).toBe(1);
    expect(stats!.prunedTraces).toBe(0); // trace 180d 未到

    const verify = harness.openReader();
    try {
      const oldCall = verify.traceStore.getCall("mc_old");
      expect(oldCall).not.toBeNull(); // call metadata 仍在
      expect(oldCall.payload_availability).toBe("expired");
      expect(verify.payloadStore.getPayloadRecords("mc_old")).toHaveLength(0);
      const newCall = verify.traceStore.getCall("mc_new");
      expect(newCall.payload_availability).toBeNull();
      expect(verify.payloadStore.getPayloadRecords("mc_new")).toHaveLength(1);
      expect(JSON.parse(fs.readFileSync(ledgerPath, "utf-8"))).toEqual({ sentinel: "untouched" });
    } finally {
      verify.close();
    }
  });

  it("trace retention：整树删除（payload→attempt→call→trace），trace 树不半删（§五十六/一百一十三）", () => {
    harness.handle.observer!.handleModelCallEvent(lifecycleEvent("mc_gone", "mt_gone", 0));
    harness.handle.sink!.handleModelCallPayloadRecord(payloadRecord("mc_gone", "mt_gone", 0));
    harness.handle.observer!.handleModelCallEvent(lifecycleEvent("mc_keep", "mt_keep", 1));
    harness.flush();
    const reader = harness.openReader();
    try {
      const old = new Date(Date.now() - 400 * 24 * 3600 * 1000).toISOString();
      reader.db.prepare(`UPDATE traces SET last_seen_at = ?, first_seen_at = ? WHERE trace_id = 'mt_gone'`).run(old, old);
    } finally {
      reader.close();
    }
    const stats = harness.handle.runMaintenance();
    expect(stats!.prunedTraces).toBe(1);
    const verify = harness.openReader();
    try {
      expect(verify.traceStore.getTrace("mt_gone")).toBeNull();
      expect(verify.traceStore.getCall("mc_gone")).toBeNull();
      expect(verify.traceStore.getAttempts("mc_gone")).toEqual([]);
      expect(verify.payloadStore.getPayloadRecords("mc_gone")).toEqual([]);
      expect(verify.traceStore.getTrace("mt_keep")).not.toBeNull();
    } finally {
      verify.close();
    }
  });
});
