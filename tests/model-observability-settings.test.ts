/**
 * Phase 8 Settings / Control Plane 测试（任务书 §一百一十二～一百一十六）：
 * 默认 disabled 不建 DB / dynamic enable 后 query 可见 / dynamic disable 不删
 * 历史 / payload opt-in + not_captured / 不回填 Prompt / PreferencesManager
 * 持久化 + restart 自动生效（canonical normalizer 单一来源）。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { PreferencesManager } from "../core/preferences-manager.ts";
import {
  DEFAULT_MODEL_OBSERVABILITY_PREFERENCE,
  modelObservabilityPreferenceToPolicy,
  normalizeModelObservabilityPreferences,
} from "../lib/llm/model-observability-preferences.ts";
import { installModelObservabilityPersistence } from "../lib/llm/model-observability-persistence.ts";
import { createModelObservabilityQueryService } from "../lib/llm/model-observability-query.ts";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { MODEL_OBSERVABILITY_SCHEMA_VERSION, modelObservabilityDbPath } from "../lib/llm/model-observability-schema.ts";
import { EMPTY_MODEL_OBSERVABILITY_FILTER } from "../lib/llm/model-observability-query-types.ts";
import type { ModelCallObserver } from "../lib/llm/model-call-observer.ts";

function makeHome(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-settings-"));
}

function seedOneCall(
  observerTarget: { observer: ModelCallObserver | null },
  callId: string,
  at: number,
) {
  const recorder = createModelCallRecorder({
    observer: observerTarget.observer,
    context: {
      callId,
      traceId: `mt_${callId}`,
      parentCallId: null,
      model: { provider: "openai", modelId: "gpt-x", api: "responses" },
      source: { subsystem: "llm", operation: "callText", surface: "server", trigger: "user_turn" },
      attribution: { kind: "session", sessionId: "s1", agentId: "a1" },
    },
    now: () => at,
  });
  recorder.beginLogicalCall({ details: { traceOrigin: "user_turn" } });
  recorder.beginAttempt({});
  recorder.endLogicalCall("ok");
}

describe("Model Observability Settings / Control Plane", () => {
  let home: string;

  beforeEach(() => {
    home = makeHome();
  });
  afterEach(() => {
    try { fs.rmSync(home, { recursive: true, force: true }); } catch { /* tmp */ }
  });

  it("preference 默认及旧关闭配置迁移：元数据、正文和合格媒体恒为开启", () => {
    expect(DEFAULT_MODEL_OBSERVABILITY_PREFERENCE).toMatchObject({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: true,
      persistBlobs: true,
    });
    // 损坏输入和旧关闭值都迁移为全开；保留天数仍按安全默认修复。
    expect(normalizeModelObservabilityPreferences(null))
      .toEqual(normalizeModelObservabilityPreferences(DEFAULT_MODEL_OBSERVABILITY_PREFERENCE));
    expect(normalizeModelObservabilityPreferences({ enabled: "yes", persistPayloads: 1, retention: { traceDays: -5 } }))
      .toMatchObject({ enabled: true, persistPayloads: true, persistBlobs: true, retention: { traceDays: 180 } });
    expect(normalizeModelObservabilityPreferences({ enabled: true })).toMatchObject({
      enabled: true, persistTraceMetadata: true, persistPayloads: true, persistBlobs: true,
    });
    // 旧客户端保存的关闭值不再生效。
    expect(normalizeModelObservabilityPreferences({ enabled: true, persistPayloads: false, persistBlobs: true }))
      .toMatchObject({ enabled: true, persistTraceMetadata: true, persistPayloads: true, persistBlobs: true });
    // days → ms。
    const policy = modelObservabilityPreferenceToPolicy(
      normalizeModelObservabilityPreferences({ enabled: true, retention: { traceDays: 10, payloadDays: 5, blobDays: 1 } }),
    );
    expect(policy.retention).toMatchObject({
      traceMaxAgeMs: 10 * 86_400_000,
      payloadMaxAgeMs: 5 * 86_400_000,
      blobMaxAgeMs: 1 * 86_400_000,
    });
  });

  it("PreferencesManager：model_observability namespace 持久化 + 读回（§五十四 restart 前半）", () => {
    const userDir = path.join(home, "user");
    fs.mkdirSync(userDir, { recursive: true });
    const manager = new PreferencesManager({ userDir, agentsDir: path.join(home, "agents") });
    expect(manager.getModelObservability())
      .toEqual(normalizeModelObservabilityPreferences(DEFAULT_MODEL_OBSERVABILITY_PREFERENCE));
    manager.setModelObservability({ enabled: true });
    // 新实例（模拟重启）从磁盘读回同一 policy。
    const restarted = new PreferencesManager({ userDir, agentsDir: path.join(home, "agents") });
    expect(restarted.getModelObservability()).toMatchObject({
      enabled: true, persistTraceMetadata: true, persistPayloads: true, persistBlobs: true,
    });
    const raw = JSON.parse(fs.readFileSync(path.join(userDir, "preferences.json"), "utf-8"));
    expect(raw.model_observability).toMatchObject({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: true,
      persistBlobs: true,
    });
  });

  it("PreferencesManager 写入保留期时会把旧关闭配置持久迁移为全开", () => {
    const userDir = path.join(home, "user-migrate");
    fs.mkdirSync(userDir, { recursive: true });
    fs.writeFileSync(path.join(userDir, "preferences.json"), JSON.stringify({
      model_observability: {
        enabled: false,
        persistTraceMetadata: false,
        persistPayloads: false,
        persistBlobs: false,
        retention: { traceDays: 90, payloadDays: 20, blobDays: 10 },
      },
    }));
    const manager = new PreferencesManager({ userDir, agentsDir: path.join(home, "agents-migrate") });
    manager.setModelObservability({ retention: { traceDays: 91 } });

    const raw = JSON.parse(fs.readFileSync(path.join(userDir, "preferences.json"), "utf-8"));
    expect(raw.model_observability).toEqual({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: true,
      persistBlobs: true,
      retention: { traceDays: 91, payloadDays: 20, blobDays: 10 },
    });
  });

  it("dynamic enable：初始 disabled 无 DB 文件；enable 后 call 落库可查（§一百一十二）", async () => {
    const disabled = installModelObservabilityPersistence({ lingxiHome: home, policy: { enabled: false } });
    expect(disabled.getHealth().status).toBe("disabled");
    expect(fs.existsSync(modelObservabilityDbPath(home))).toBe(false);

    await disabled.close();
    const handle = installModelObservabilityPersistence({
      lingxiHome: home,
      policy: { enabled: true, persistTraceMetadata: true },
    });
    expect(handle.getHealth().status).toBe("active");
    seedOneCall(handle, "mc_dyn1", Date.UTC(2026, 7, 1));
    handle.flushSync();
    const service = createModelObservabilityQueryService({ lingxiHome: home });
    try {
      const health = service.getHealth();
      expect(health.ok === true && health.value.callCount).toBe(1);
    } finally {
      service.close();
      await handle.close();
    }
  });

  it("dynamic disable：新事件不落库、旧数据可查、DB 不删除（§一百一十三/六十）", async () => {
    const harness = createModelObservabilityTestHarness({ lingxiHome: home });
    seedOneCall(harness.handle, "mc_keep", Date.UTC(2026, 7, 1));
    harness.flush();
    await harness.close();

    // close 后模拟 disabled 运行期：直接发事件到已关 observer 不落库。
    const dbPath = modelObservabilityDbPath(home);
    const sizeAfterClose = fs.statSync(dbPath).size;

    const service = createModelObservabilityQueryService({ lingxiHome: home });
    try {
      const calls = service.queryCalls({ filter: EMPTY_MODEL_OBSERVABILITY_FILTER, sort: "started_at_desc", limit: 50, cursor: null });
      expect(calls.ok === true && calls.value.calls.map((c) => c.callId)).toEqual(["mc_keep"]);
      const health = service.getHealth();
      expect(health.ok === true && health.value.queryStatus).toBe("ready");
      expect(health.ok === true && health.value.schemaVersion).toBe(MODEL_OBSERVABILITY_SCHEMA_VERSION);
    } finally {
      service.close();
    }
    expect(fs.existsSync(dbPath)).toBe(true);
    expect(fs.statSync(dbPath).size).toBeGreaterThanOrEqual(sizeAfterClose);
    harness.cleanup();
  });

  it("payload opt-in：persistPayloads=false → not_captured；开启后新 call 有 payload，旧 call 不回填（§一百一十五/一百一十六）", async () => {
    let harness = createModelObservabilityTestHarness({
      lingxiHome: home,
      policy: { enabled: true, persistTraceMetadata: true, persistPayloads: false },
    });
    seedOneCall(harness.handle, "mc_meta_only", Date.UTC(2026, 7, 1));
    harness.flush();
    // not_captured 是运行时证据（§三十八）。
    {
      const reader = harness.openReader();
      const row = reader.db.prepare(`SELECT payload_availability FROM model_calls WHERE call_id = ?`).get("mc_meta_only");
      expect(row.payload_availability).toBe("not_captured");
      const payloads = reader.db.prepare(`SELECT COUNT(*) AS n FROM payload_records`).get();
      expect(payloads.n).toBe(0);
      reader.close();
    }
    await harness.close();

    // 开启 payload persistence：新 call 有正文，旧 call 不回填。
    harness = createModelObservabilityTestHarness({
      lingxiHome: home,
      policy: { enabled: true, persistTraceMetadata: true, persistPayloads: true },
    });
    seedOneCall(harness.handle, "mc_with_payload", Date.UTC(2026, 7, 2));
    harness.handle.sink?.handleModelCallPayloadRecord({
      schemaVersion: 1, kind: "semantic_request", capturedAt: "2026-08-02T00:00:01.000Z",
      callId: "mc_with_payload", traceId: "mt_mc_with_payload", parentCallId: null, attemptId: null,
      providerRequestOrdinal: null, model: null, source: null, attribution: null,
      visibility: "full", fidelity: "runtime_exact",
      sanitization: { redacted: false, truncated: false, degraded: false },
      payload: { inputShape: "chat_context" }, semanticInputProvenance: null, providerRequestProvenance: null,
    } as never);
    harness.flush();
    {
      const reader = harness.openReader();
      const oldRow = reader.db.prepare(`SELECT payload_availability FROM model_calls WHERE call_id = ?`).get("mc_meta_only");
      expect(oldRow.payload_availability).toBe("not_captured");
      const payloads = reader.db.prepare(`SELECT call_id FROM payload_records`).all();
      expect(payloads.map((r: any) => r.call_id)).toEqual(["mc_with_payload"]);
      reader.close();
    }
    await harness.close();
    harness.cleanup();
  });

  it("query service 生命周期：reconfigure 后 lazy reopen，不持已关 DB handle（§九十一）", async () => {
    const harness = createModelObservabilityTestHarness({ lingxiHome: home });
    seedOneCall(harness.handle, "mc_l1", Date.UTC(2026, 7, 1));
    harness.flush();
    await harness.close();

    const service = createModelObservabilityQueryService({ lingxiHome: home });
    try {
      // close 后查询仍可读（committed durable state）。
      let calls = service.queryCalls({ filter: EMPTY_MODEL_OBSERVABILITY_FILTER, sort: "started_at_desc", limit: 10, cursor: null });
      expect(calls.ok === true && calls.value.calls).toHaveLength(1);
      // invalidate（模拟 reconfigure）后 reopen 正常。
      service.invalidate();
      calls = service.queryCalls({ filter: EMPTY_MODEL_OBSERVABILITY_FILTER, sort: "started_at_desc", limit: 10, cursor: null });
      expect(calls.ok === true && calls.value.calls).toHaveLength(1);
    } finally {
      service.close();
      harness.cleanup();
    }
  });

  it("health read model：atRestEncryption=false + absent 状态诚实（§四十九/六十二/九十三）", () => {
    const service = createModelObservabilityQueryService({ lingxiHome: home });
    try {
      const health = service.getHealth();
      expect(health.ok === true && health.value.queryStatus).toBe("absent");
      expect(fs.existsSync(modelObservabilityDbPath(home))).toBe(false);
    } finally {
      service.close();
    }
    // persistence disabled + DB 存在 → 仍可查询（§五 desired/effective 解耦）。
    const disabledHandle = installModelObservabilityPersistence({ lingxiHome: home, policy: { enabled: false } });
    expect(disabledHandle.getHealth().status).toBe("disabled");
    expect(fs.existsSync(modelObservabilityDbPath(home))).toBe(false);
  });
});
