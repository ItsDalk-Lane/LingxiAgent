/**
 * Phase 8 Export 测试（任务书 §一百一十七～一百一十九）：
 * metadata-only / filtered / includePayloads / limit / opaque-unavailable 保留 /
 * usage metrics / identity / 独立 schema version / 毒丸不出现 / streaming。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createModelObservabilityQueryService } from "../lib/llm/model-observability-query.ts";
import { normalizeModelObservabilityQuery } from "../lib/llm/model-observability-query-types.ts";
import {
  MODEL_OBSERVABILITY_EXPORT_DEFAULT_MAX_CALLS,
  MODEL_OBSERVABILITY_EXPORT_SCHEMA_VERSION,
  normalizeModelObservabilityExportOptions,
  startModelObservabilityExport,
} from "../lib/llm/model-observability-export.ts";

const POISONS = ["sk-live-poison", "Bearer eyJhbGciOi", "Set-Cookie: session=", "PRIVATE KEY-----", "X-Amz-Signature="];

function seedCall(
  harness: ReturnType<typeof createModelObservabilityTestHarness>,
  callId: string,
  at: number,
  options: { subsystem?: string; withPayload?: boolean } = {},
) {
  const recorder = createModelCallRecorder({
    observer: harness.handle.observer,
    context: {
      callId,
      traceId: `mt_${callId}`,
      parentCallId: null,
      model: { provider: "openai", modelId: "gpt-x", api: "responses" },
      source: {
        subsystem: options.subsystem ?? "llm",
        operation: "callText",
        surface: "server",
        trigger: "user_turn",
      },
      attribution: { kind: "session", sessionId: "s1", agentId: "a1" },
    },
    now: () => at,
  });
  recorder.beginLogicalCall({ details: { traceOrigin: "user_turn" } });
  recorder.beginAttempt({});
  recorder.endLogicalCall("ok");
  if (options.withPayload !== false) {
    harness.handle.sink?.handleModelCallPayloadRecord({
      schemaVersion: 1,
      kind: "semantic_request",
      capturedAt: new Date(at + 1000).toISOString(),
      callId,
      traceId: `mt_${callId}`,
      parentCallId: null,
      attemptId: null,
      providerRequestOrdinal: null,
      model: null, source: null, attribution: null,
      visibility: "full",
      fidelity: "runtime_exact",
      sanitization: { redacted: true, truncated: false, degraded: false },
      payload: { inputShape: "chat_context", messages: [{ role: "user", content: "hello world" }] },
      semanticInputProvenance: null,
      providerRequestProvenance: null,
    } as never);
  }
}

function seedOpaqueRecord(
  harness: ReturnType<typeof createModelObservabilityTestHarness>,
  callId: string,
  at: number,
) {
  harness.handle.sink?.handleModelCallPayloadRecord({
    schemaVersion: 1,
    kind: "provider_request",
    capturedAt: new Date(at + 2000).toISOString(),
    callId,
    traceId: `mt_${callId}`,
    parentCallId: null, attemptId: null, providerRequestOrdinal: null,
    model: null, source: null, attribution: null,
    visibility: "opaque",
    fidelity: "opaque",
    sanitization: { redacted: false, truncated: false, degraded: false },
    payload: null,
    semanticInputProvenance: null,
    providerRequestProvenance: null,
  } as never);
}

async function collectExport(start: ReturnType<typeof startModelObservabilityExport>): Promise<string[]> {
  if (start.kind !== "ready") throw new Error(`export not ready: ${start.kind}`);
  const lines: string[] = [];
  for await (const line of start.iterate()) {
    lines.push(line.trim());
  }
  return lines;
}

describe("Model Observability Export", () => {
  let home: string;
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;
  let service: ReturnType<typeof createModelObservabilityQueryService>;

  beforeEach(() => {
    home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-export-"));
    harness = createModelObservabilityTestHarness({ lingxiHome: home });
  });
  afterEach(async () => {
    service?.close?.();
    await harness.close();
    harness.cleanup();
  });

  function wireUsage(entries: unknown[]) {
    harness.handle.initializeAccounting({
      listLedgerEntries: () => entries,
      subscribeUsage: () => () => { /* 测试无 live 流 */ },
    });
    harness.flush();
  }

  function startExport(body: Record<string, unknown>) {
    const normalizedQuery = normalizeModelObservabilityQuery(body.query ?? {});
    if (normalizedQuery.ok === false) throw new Error(normalizedQuery.error.message);
    const options = normalizeModelObservabilityExportOptions(body);
    if (options.ok === false) throw new Error(options.error.message);
    return startModelObservabilityExport(service, normalizedQuery.value, options.value);
  }

  it("metadata-only：manifest 首行 + 每 call 一行，无正文（§一百一十七/七十五）", async () => {
    seedCall(harness, "mc_e1", Date.UTC(2026, 7, 1));
    seedCall(harness, "mc_e2", Date.UTC(2026, 7, 2));
    wireUsage([
      {
        schemaVersion: 1, requestId: "llm_e1", startedAt: "2026-08-01T00:00:00.000Z",
        endedAt: "2026-08-01T00:00:01.000Z", durationMs: 1000, status: "ok",
        source: { subsystem: "llm" }, attribution: {}, metadata: { modelCallId: "mc_e1" },
        model: { provider: "openai" },
        usage: { costTotal: 0.01, input: { totalTokens: 10, uncachedTokens: 10 }, output: { totalTokens: 5 }, cache: { readTokens: 0, writeTokens: 0, hit: false, created: false, support: "reported" }, totalTokens: 15 },
        rawUsageShape: null, error: null,
      },
    ]);
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const lines = await collectExport(startExport({}));
    expect(lines).toHaveLength(3);
    const manifest = JSON.parse(lines[0]);
    expect(manifest).toMatchObject({
      type: "manifest",
      exportSchemaVersion: MODEL_OBSERVABILITY_EXPORT_SCHEMA_VERSION,
      includePayloads: false,
      totalCalls: 2,
      storageSchemaVersion: 4,
      backfillSource: "bounded_usage_ledger",
    });
    const bundle1 = JSON.parse(lines[1]);
    expect(bundle1).toMatchObject({ type: "model_call", schemaVersion: MODEL_OBSERVABILITY_EXPORT_SCHEMA_VERSION });
    expect(bundle1.call.callId).toBe("mc_e2"); // started_at DESC
    // usage metrics 随 bundle（§一百一十七）。
    const withUsage = [bundle1, JSON.parse(lines[2])].find((b: any) => b.call.callId === "mc_e1");
    expect(withUsage.usage).toMatchObject({ availability: "present", status: "ok", summary: { inputTokens: 10 } });
    // trace identity（§一百一十七）。
    expect(bundle1.call.traceId).toBe("mt_mc_e2");
    expect(bundle1.trace.traceId).toBe("mt_mc_e2");
    // metadata-only：payload 是 metadata（无 payload 字段/正文）。
    for (const payload of bundle1.payloads) {
      expect(payload).not.toHaveProperty("payload");
      expect(payload.hasBody).toBe(true);
    }
    // 毒丸不出现（§一百一十八）——export 字节里没有任何秘密标记。
    const raw = lines.join("\n");
    for (const poison of POISONS) expect(raw).not.toContain(poison);
  });

  it("filtered export：filter 生效（同一 Filter Contract，§七十八）", async () => {
    seedCall(harness, "mc_f1", Date.UTC(2026, 7, 1), { subsystem: "llm" });
    seedCall(harness, "mc_f2", Date.UTC(2026, 7, 2), { subsystem: "memory" });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const lines = await collectExport(startExport({ query: { filter: { subsystem: "memory" } } }));
    expect(lines).toHaveLength(2);
    const bundle = JSON.parse(lines[1]);
    expect(bundle.call.callId).toBe("mc_f2");
  });

  it("includePayloads：sanitized 正文导出；opaque 记录保持不可用（§七十六/一百零七）", async () => {
    seedCall(harness, "mc_p1", Date.UTC(2026, 7, 1));
    seedOpaqueRecord(harness, "mc_p1", Date.UTC(2026, 7, 1));
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const lines = await collectExport(startExport({ includePayloads: true }));
    const bundle = JSON.parse(lines[1]);
    expect(bundle.call.callId).toBe("mc_p1");
    const bodies = bundle.payloads;
    expect(bodies).toHaveLength(2);
    const semantic = bodies.find((p: any) => p.kind === "semantic_request");
    expect(semantic.contentAvailable).toBe(true);
    expect(semantic.payload).toMatchObject({ inputShape: "chat_context" });
    expect(semantic.redacted).toBe(true);
    const opaque = bodies.find((p: any) => p.visibility === "opaque");
    expect(opaque.contentAvailable).toBe(false);
    expect(opaque.contentState).toBe("opaque_or_unavailable");
    expect(opaque.payload).toBeNull();
    // 没有 includeRaw 这种选项（§七十六）：显式 unknown_field 拒绝。
    expect(normalizeModelObservabilityExportOptions({ includeRaw: true }).ok).toBe(false);
  });

  it("损坏的 usage/payload 状态原样进入 export，不被改写成零或空正文", async () => {
    seedCall(harness, "mc_corrupt_export", Date.UTC(2026, 7, 1));
    harness.flush();
    const reader = harness.openReader();
    try {
      reader.db.prepare(`INSERT INTO model_call_usage (
        model_call_id, usage_status, input_total_tokens, total_tokens, cost_total,
        created_at, updated_at
      ) VALUES ('mc_corrupt_export', 'ok', 'broken', 'broken', 'broken',
        '2026-08-01T00:00:00.000Z', '2026-08-01T00:00:00.000Z')`).run();
      reader.db.prepare("UPDATE payload_records SET payload_json = '{broken' WHERE call_id = ?")
        .run("mc_corrupt_export");
    } finally {
      reader.close();
    }
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const lines = await collectExport(startExport({ includePayloads: true }));
    const bundle = JSON.parse(lines[1]);
    expect(bundle.usage).toEqual({ availability: "corrupt", status: "ok", summary: null });
    expect(bundle.payloads[0]).toMatchObject({
      contentAvailable: false,
      contentState: "corrupt",
      payload: null,
    });
  });

  it("export limit：超 maxCalls → limit_error（§八十一）", () => {
    for (let i = 0; i < 5; i++) {
      seedCall(harness, `mc_l${i}`, Date.UTC(2026, 7, 1, i));
    }
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const start = startExport({ maxCalls: 3 });
    expect(start.kind).toBe("limit_error");
    if (start.kind === "limit_error") {
      expect(start.matchedCalls).toBe(5);
      expect(start.maxCalls).toBe(3);
    }
  });

  it("absent store → absent；默认 maxCalls = 50k（§九十二/八十一）", () => {
    // 独立空 home（beforeEach 的 harness 已创建 store，这里验证真正的 absent）。
    const emptyHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-export-absent-"));
    try {
      const absentService = createModelObservabilityQueryService({ lingxiHome: emptyHome });
      const normalizedQuery = normalizeModelObservabilityQuery({});
      const options = normalizeModelObservabilityExportOptions({});
      if (normalizedQuery.ok !== true || options.ok !== true) throw new Error("normalize failed");
      const start = startModelObservabilityExport(absentService, normalizedQuery.value, options.value);
      expect(start.kind).toBe("absent");
      expect(options.value.maxCalls).toBe(MODEL_OBSERVABILITY_EXPORT_DEFAULT_MAX_CALLS);
      expect(options.value.includePayloads).toBe(false);
      absentService.close();
    } finally {
      try { fs.rmSync(emptyHome, { recursive: true, force: true }); } catch { /* tmp */ }
    }
  });

  it("streaming：300 calls 分页导出，行数 = manifest + 300（§一百一十九）", async () => {
    for (let i = 0; i < 300; i++) {
      seedCall(harness, `mc_s${String(i).padStart(3, "0")}`, Date.UTC(2026, 7, 1) + i * 1000);
      if (i % 100 === 99) harness.flush();
    }
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const lines = await collectExport(startExport({}));
    expect(lines).toHaveLength(301);
    const manifest = JSON.parse(lines[0]);
    expect(manifest.totalCalls).toBe(300);
    // 无重复、无遗漏。
    const ids = lines.slice(1).map((l) => JSON.parse(l).call.callId);
    expect(new Set(ids).size).toBe(300);
  });
});
