/**
 * Phase 10.1 AR-15：运行中切换 recording policy 时按 generation 排水。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { LingxiEngine } from "../core/engine.ts";
import { callText } from "../core/llm-client.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import {
  createModelCallPayloadCaptureSession,
  setModelCallBlobExternalizer,
  setModelCallPayloadSink,
} from "../lib/llm/model-call-payload-capture.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import {
  createModelObservabilityGenerationManager,
  type ModelObservabilityGenerationManager,
} from "../lib/llm/model-observability-engine.ts";
import {
  loadBetterSqliteDatabase,
  modelObservabilityDbPath,
} from "../lib/llm/model-observability-schema.ts";
import {
  modelObservabilityPreferenceToPolicy,
  normalizeModelObservabilityPreferences,
} from "../lib/llm/model-observability-preferences.ts";

const MODEL = { provider: "openai", modelId: "gpt-generation", api: "openai-completions" };
const SOURCE = { subsystem: "test", operation: "generation", surface: "server", trigger: "test" };

describe("Model Observability Generation + Drain", () => {
  let home: string;
  let manager: ModelObservabilityGenerationManager | null;

  beforeEach(() => {
    home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-generation-"));
    manager = null;
    setModelCallObserver(null);
    setModelCallPayloadSink(null);
    setModelCallBlobExternalizer(null);
  });

  afterEach(async () => {
    await manager?.close();
    setModelCallObserver(null);
    setModelCallPayloadSink(null);
    setModelCallBlobExternalizer(null);
    vi.unstubAllGlobals();
    fs.rmSync(home, { recursive: true, force: true });
  });

  it("mid-flight reconfigure：旧 Call 落旧代，新 Call 落新代，且不 incomplete/duplicate", async () => {
    manager = createModelObservabilityGenerationManager({
      lingxiHome: home,
      drainTimeoutMs: 1_000,
    });
    const generationA = manager.reconfigure({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: false,
      flushIntervalMs: 60_000,
    });
    expect(generationA?.getHealth().status).toBe("active");

    const oldRecorder = createModelCallRecorder({
      context: {
        callId: "mc_generation_old",
        traceId: "mt_generation_old",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    const oldCapture = createModelCallPayloadCaptureSession({
      callId: oldRecorder.callId,
      traceId: oldRecorder.traceId,
      parentCallId: null,
      model: MODEL,
      source: SOURCE,
      attribution: { kind: "test" },
    });
    expect(oldCapture).toBeNull();
    oldRecorder.beginLogicalCall();
    oldRecorder.beginAttempt();
    oldRecorder.providerRequestPrepared({ details: { protocol: "openai-completions" } });
    expect(manager.getState().activeCalls).toBe(1);
    expect(generationA?.getHealth().queuedTraceEvents).toBe(3);
    generationA?.flushSync();

    const generationB = manager.reconfigure({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: true,
      flushIntervalMs: 60_000,
    });
    expect(generationA?.getHealth().status).toBe("active");
    expect(generationB?.getHealth().status).toBe("active");

    oldRecorder.providerResponseReceived({ httpStatus: 200 });
    oldRecorder.semanticResponseCompleted({ details: { hasText: true } });
    oldRecorder.endLogicalCall("ok");

    const newRecorder = createModelCallRecorder({
      context: {
        callId: "mc_generation_new",
        traceId: "mt_generation_new",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    const newCapture = createModelCallPayloadCaptureSession({
      callId: newRecorder.callId,
      traceId: newRecorder.traceId,
      parentCallId: null,
      model: MODEL,
      source: SOURCE,
      attribution: { kind: "test" },
    });
    expect(newCapture).not.toBeNull();
    newRecorder.attachPayloadCapture(newCapture);
    newRecorder.beginLogicalCall();
    newRecorder.beginAttempt();
    newCapture?.captureSemanticRequest({
      inputShape: "calltext",
      systemPrompt: "new generation payload",
      messages: [],
    });
    newRecorder.endLogicalCall("ok");

    await manager.waitForRetired();
    generationB?.flushSync();
    expect(generationA?.getHealth().status).toBe("closed");

    const Database = loadBetterSqliteDatabase();
    const db = new Database(modelObservabilityDbPath(home), { readonly: true });
    try {
      const calls = db.prepare(
        `SELECT call_id, terminal_status, interrupted_by_restart, payload_availability
         FROM model_calls ORDER BY call_id`,
      ).all();
      expect(calls).toEqual([
        {
          call_id: "mc_generation_new",
          terminal_status: "ok",
          interrupted_by_restart: 0,
          payload_availability: null,
        },
        {
          call_id: "mc_generation_old",
          terminal_status: "ok",
          interrupted_by_restart: 0,
          payload_availability: "not_captured",
        },
      ]);
      expect(db.prepare(`SELECT call_id, COUNT(*) AS n FROM payload_records GROUP BY call_id`).all())
        .toEqual([{ call_id: "mc_generation_new", n: 1 }]);
      expect(db.prepare(`SELECT call_id, COUNT(*) AS n FROM model_calls GROUP BY call_id HAVING n > 1`).all())
        .toEqual([]);
    } finally {
      db.close();
    }
  });

  it("retired generation 对永不结束的 Call 使用有界排水超时", async () => {
    manager = createModelObservabilityGenerationManager({
      lingxiHome: home,
      drainTimeoutMs: 25,
    });
    const generationA = manager.reconfigure({ enabled: true, persistTraceMetadata: true });
    const recorder = createModelCallRecorder({
      context: {
        callId: "mc_never_ends",
        traceId: "mt_never_ends",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    recorder.beginLogicalCall();
    manager.reconfigure({ enabled: true, persistTraceMetadata: true });

    await manager.waitForRetired();
    expect(generationA?.getHealth().status).toBe("closed");
  });

  it("payload → metadata：在途旧 Call 仍写旧代 payload，新 Call 只写 metadata", async () => {
    manager = createModelObservabilityGenerationManager({ lingxiHome: home, drainTimeoutMs: 1_000 });
    const generationA = manager.reconfigure({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: true,
      flushIntervalMs: 60_000,
    });
    const oldRecorder = createModelCallRecorder({
      context: {
        callId: "mc_payload_old",
        traceId: "mt_payload_old",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    const oldCapture = createModelCallPayloadCaptureSession({
      callId: oldRecorder.callId,
      traceId: oldRecorder.traceId,
      parentCallId: null,
      model: MODEL,
      source: SOURCE,
      attribution: { kind: "test" },
    });
    expect(oldCapture).not.toBeNull();
    oldRecorder.attachPayloadCapture(oldCapture);
    oldRecorder.beginLogicalCall();
    oldRecorder.beginAttempt();
    generationA?.flushSync();

    const generationB = manager.reconfigure({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: false,
      flushIntervalMs: 60_000,
    });
    oldCapture?.captureSemanticRequest({
      inputShape: "calltext",
      systemPrompt: "old generation payload",
      messages: [],
    });
    oldRecorder.endLogicalCall("ok");

    const newRecorder = createModelCallRecorder({
      context: {
        callId: "mc_metadata_new",
        traceId: "mt_metadata_new",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    expect(createModelCallPayloadCaptureSession({
      callId: newRecorder.callId,
      traceId: newRecorder.traceId,
      parentCallId: null,
      model: MODEL,
      source: SOURCE,
      attribution: { kind: "test" },
    })).toBeNull();
    newRecorder.beginLogicalCall();
    newRecorder.beginAttempt();
    newRecorder.endLogicalCall("ok");

    await manager.waitForRetired();
    generationB?.flushSync();

    const Database = loadBetterSqliteDatabase();
    const db = new Database(modelObservabilityDbPath(home), { readonly: true });
    try {
      expect(db.prepare(
        `SELECT call_id, terminal_status, interrupted_by_restart, payload_availability
         FROM model_calls ORDER BY call_id`,
      ).all()).toEqual([
        {
          call_id: "mc_metadata_new",
          terminal_status: "ok",
          interrupted_by_restart: 0,
          payload_availability: "not_captured",
        },
        {
          call_id: "mc_payload_old",
          terminal_status: "ok",
          interrupted_by_restart: 0,
          payload_availability: null,
        },
      ]);
      expect(db.prepare(`SELECT call_id, COUNT(*) AS n FROM payload_records GROUP BY call_id`).all())
        .toEqual([{ call_id: "mc_payload_old", n: 1 }]);
    } finally {
      db.close();
    }
  });

  it("enabled → disabled：在途旧 Call 写完，新 Call 不再进入存储", async () => {
    manager = createModelObservabilityGenerationManager({ lingxiHome: home, drainTimeoutMs: 1_000 });
    const generationA = manager.reconfigure({
      enabled: true,
      persistTraceMetadata: true,
      flushIntervalMs: 60_000,
    });
    const oldRecorder = createModelCallRecorder({
      context: {
        callId: "mc_enabled_old",
        traceId: "mt_enabled_old",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    oldRecorder.beginLogicalCall();
    oldRecorder.beginAttempt();
    generationA?.flushSync();

    expect(manager.reconfigure(null)).toBeNull();
    expect(manager.current).toBeNull();

    const disabledRecorder = createModelCallRecorder({
      context: {
        callId: "mc_disabled_new",
        traceId: "mt_disabled_new",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    disabledRecorder.beginLogicalCall();
    disabledRecorder.beginAttempt();
    disabledRecorder.endLogicalCall("ok");
    oldRecorder.endLogicalCall("ok");

    await manager.waitForRetired();
    expect(generationA?.getHealth().status).toBe("closed");

    const Database = loadBetterSqliteDatabase();
    const db = new Database(modelObservabilityDbPath(home), { readonly: true });
    try {
      expect(db.prepare(
        `SELECT call_id, terminal_status, interrupted_by_restart
         FROM model_calls ORDER BY call_id`,
      ).all()).toEqual([
        { call_id: "mc_enabled_old", terminal_status: "ok", interrupted_by_restart: 0 },
      ]);
    } finally {
      db.close();
    }
  });

  it("enabled policy A → B：设置只影响新 Call，两个代际各自完整且不重复", async () => {
    manager = createModelObservabilityGenerationManager({ lingxiHome: home, drainTimeoutMs: 1_000 });
    const generationA = manager.reconfigure({
      enabled: true,
      persistTraceMetadata: true,
      retention: { traceMaxAgeMs: 10 * 86_400_000 },
      flushIntervalMs: 60_000,
    });
    const oldRecorder = createModelCallRecorder({
      context: {
        callId: "mc_policy_a",
        traceId: "mt_policy_a",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    oldRecorder.beginLogicalCall();
    oldRecorder.beginAttempt();
    generationA?.flushSync();

    const generationB = manager.reconfigure({
      enabled: true,
      persistTraceMetadata: true,
      retention: { traceMaxAgeMs: 20 * 86_400_000 },
      flushIntervalMs: 60_000,
    });
    expect(generationA?.policy.retention.traceMaxAgeMs).toBe(10 * 86_400_000);
    expect(generationB?.policy.retention.traceMaxAgeMs).toBe(20 * 86_400_000);
    expect(manager.getState().retiringGenerations).toBe(1);

    oldRecorder.endLogicalCall("ok");
    const newRecorder = createModelCallRecorder({
      context: {
        callId: "mc_policy_b",
        traceId: "mt_policy_b",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    newRecorder.beginLogicalCall();
    newRecorder.beginAttempt();
    newRecorder.endLogicalCall("ok");

    await manager.waitForRetired();
    generationB?.flushSync();

    const Database = loadBetterSqliteDatabase();
    const db = new Database(modelObservabilityDbPath(home), { readonly: true });
    try {
      expect(db.prepare(
        `SELECT call_id, terminal_status, interrupted_by_restart
         FROM model_calls ORDER BY call_id`,
      ).all()).toEqual([
        { call_id: "mc_policy_a", terminal_status: "ok", interrupted_by_restart: 0 },
        { call_id: "mc_policy_b", terminal_status: "ok", interrupted_by_restart: 0 },
      ]);
      expect(db.prepare(`SELECT call_id, COUNT(*) AS n FROM model_calls GROUP BY call_id HAVING n > 1`).all())
        .toEqual([]);
    } finally {
      db.close();
    }
  });

  it("core settings 真实入口使用 generation manager，不再先关闭旧 handle", async () => {
    manager = createModelObservabilityGenerationManager({ lingxiHome: home, drainTimeoutMs: 1_000 });
    let desired = normalizeModelObservabilityPreferences({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: false,
    });
    const engine = Object.create(LingxiEngine.prototype) as LingxiEngine;
    engine.lingxiHome = home;
    engine._modelObservability = null;
    engine._modelObservabilityGenerations = manager;
    engine._usageLedger = null;
    engine._prefs = {
      getModelObservability: () => desired,
      setModelObservability: (patch: Record<string, unknown>) => {
        desired = normalizeModelObservabilityPreferences({ ...desired, ...patch });
        return desired;
      },
    };
    engine._modelObservabilityQuery = {
      invalidate: vi.fn(),
      getHealth: () => ({ ok: false, error: { code: "not_initialized" } }),
    };
    engine._installModelObservability(modelObservabilityPreferenceToPolicy(desired));
    const generationA = engine.modelObservabilityPersistence;

    const recorder = createModelCallRecorder({
      context: {
        callId: "mc_core_settings",
        traceId: "mt_core_settings",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "test" },
      },
    });
    recorder.beginLogicalCall();
    recorder.beginAttempt();
    generationA.flushSync();

    await engine.setModelObservabilitySettings({ persistPayloads: true });
    expect(engine.modelObservabilityPersistence).not.toBe(generationA);
    expect(generationA.getHealth().status).toBe("active");
    expect(engine._modelObservabilityQuery.invalidate).toHaveBeenCalledOnce();

    recorder.endLogicalCall("ok");
    await manager.waitForRetired();
    expect(generationA.getHealth().status).toBe("closed");
  });

  it("delayed fake provider E2E：响应期间换代不截断生产 callText 生命周期", async () => {
    manager = createModelObservabilityGenerationManager({ lingxiHome: home, drainTimeoutMs: 1_000 });
    const generationA = manager.reconfigure({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: false,
      flushIntervalMs: 60_000,
    });

    let releaseProvider: ((response: Response) => void) | null = null;
    const delayedFetch = vi.fn(() => new Promise<Response>((resolve) => {
      releaseProvider = resolve;
    }));
    vi.stubGlobal("fetch", delayedFetch);
    const firstCall = callText({
      api: "openai-completions",
      apiKey: "test-key",
      baseUrl: "https://generation.test/v1",
      model: { id: "gpt-generation", provider: "openai" },
      systemPrompt: "system",
      messages: [{ role: "user", content: "first" }],
      usageContext: {
        source: SOURCE,
        attribution: { kind: "test" },
      },
    } as never);
    expect(delayedFetch).toHaveBeenCalledOnce();
    expect(manager.getState().activeCalls).toBe(1);
    generationA?.flushSync();

    const generationB = manager.reconfigure({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: true,
      flushIntervalMs: 60_000,
    });
    releaseProvider?.(new Response(JSON.stringify({
      choices: [{ message: { content: "first reply" }, finish_reason: "stop" }],
      usage: { prompt_tokens: 2, completion_tokens: 1, total_tokens: 3 },
    }), { status: 200, headers: { "content-type": "application/json" } }));
    await expect(firstCall).resolves.toBe("first reply");
    await manager.waitForRetired();
    expect(generationA?.getHealth().status).toBe("closed");

    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      choices: [{ message: { content: "second reply" }, finish_reason: "stop" }],
      usage: { prompt_tokens: 2, completion_tokens: 1, total_tokens: 3 },
    }), { status: 200, headers: { "content-type": "application/json" } })));
    await expect(callText({
      api: "openai-completions",
      apiKey: "test-key",
      baseUrl: "https://generation.test/v1",
      model: { id: "gpt-generation", provider: "openai" },
      systemPrompt: "system",
      messages: [{ role: "user", content: "second" }],
      usageContext: {
        source: SOURCE,
        attribution: { kind: "test" },
      },
    } as never)).resolves.toBe("second reply");
    generationB?.flushSync();

    const Database = loadBetterSqliteDatabase();
    const db = new Database(modelObservabilityDbPath(home), { readonly: true });
    try {
      expect(db.prepare(
        `SELECT COUNT(*) AS calls,
                COUNT(DISTINCT call_id) AS distinct_calls,
                SUM(CASE WHEN terminal_status = 'ok' THEN 1 ELSE 0 END) AS ok_calls,
                SUM(interrupted_by_restart) AS interrupted_calls
         FROM model_calls`,
      ).get()).toEqual({ calls: 2, distinct_calls: 2, ok_calls: 2, interrupted_calls: 0 });
      expect(db.prepare(
        `SELECT mc.payload_availability, COUNT(pr.id) AS payload_records
         FROM model_calls mc
         LEFT JOIN payload_records pr ON pr.call_id = mc.call_id
         GROUP BY mc.call_id
         ORDER BY payload_records`,
      ).all()).toEqual([
        { payload_availability: "not_captured", payload_records: 0 },
        { payload_availability: null, payload_records: 4 },
      ]);
    } finally {
      db.close();
    }
  });
});
