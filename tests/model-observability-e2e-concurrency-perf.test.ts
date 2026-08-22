/**
 * Phase 10 E2E Truth — 并发隔离（S20）与延迟开销边界（S37）。
 *
 * S20：两个独立 chat session 并行真实 HTTP 调用——traceId/parent/payload/usage
 *      不串（§五十八）。
 * S37：Observability ON/OFF 的宽松 stress（100 calls × {off, payload-on}）——
 *      关注数量级回归，不做微秒 SLA（§一百零三）。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { callText } from "../core/llm-client.ts";
import {
  createScenarioHarness,
  flushAsync,
  openaiCompletionsJson,
  type ScenarioHarness,
} from "./helpers/model-observability-scenario-harness.ts";

const POISON_KEY = "sk-E2E-PERF-WITNESS-POISON-90d4c8ab";

let harness: ScenarioHarness;

beforeEach(async () => {
  harness = await createScenarioHarness();
});
afterEach(async () => {
  await harness.close();
  harness.cleanup();
});

function placeCall(subsystem: string, session: string) {
  harness.witness.scriptNext({ kind: "json", body: openaiCompletionsJson({ content: "R" }) });
  return callText({
    api: "openai-completions",
    apiKey: POISON_KEY,
    baseUrl: harness.witness.baseUrl,
    model: { id: "witness-model", provider: "witness-provider" } as any,
    systemPrompt: "S",
    messages: [{ role: "user", content: `U-${session}` }],
    usageContext: {
      source: { subsystem, operation: "concurrency", surface: "desktop", trigger: "user" },
      attribution: { kind: "session", sessionId: session },
    },
  } as any);
}

describe("E2E truth — 并发隔离（S20）", () => {
  it("两个 session 并行：trace 不串、payload 不串、归属不串", async () => {
    await Promise.all([
      placeCall("session", "sess-A"),
      placeCall("session", "sess-B"),
    ]);
    await flushAsync(5);
    harness.flush();
    await flushAsync(3);

    const callIds = harness.observer!.callIds();
    expect(callIds).toHaveLength(2);
    const identityA = harness.observer!.callIdentity(callIds[0])!;
    const identityB = harness.observer!.callIdentity(callIds[1])!;
    // 独立任务：不同 trace、互不为 parent
    expect(identityA.traceId).not.toBe(identityB.traceId);
    expect(identityA.parentCallId).toBeNull();
    expect(identityB.parentCallId).toBeNull();
    harness.observer!.assertTraceGraphValid();

    // durable 归属不串：query 按 session 过滤各得其一
    const query = harness.query();
    const { normalizeModelObservabilityQuery } = await import("../lib/llm/model-observability-query-types.ts");
    for (const session of ["sess-A", "sess-B"]) {
      const normalized = normalizeModelObservabilityQuery({ filter: { sessionId: session } });
      if (normalized.ok === false) throw new Error(normalized.error.message);
      const page = query.queryCalls(normalized.value);
      expect(page.ok).toBe(true);
      if (!page.ok) return;
      expect(page.value.calls).toHaveLength(1);
      expect(page.value.calls[0].attribution.sessionId).toBe(session);
    }
  });
});

describe("E2E truth — 延迟开销边界（S37）", () => {
  it("100 calls：OFF vs Payload-ON 无数量级回归（宽松阈值 ×5）", async () => {
    // OFF：卸载 observer/persistence
    await harness.handle.uninstall();
    const offStart = Date.now();
    for (let i = 0; i < 100; i++) {
      await placeCall("memory", "sess-perf");
    }
    const offMs = Date.now() - offStart;

    // ON：重新安装（同 HOME 会拿到既有 DB——独立性不重要，只测开销）
    const { installModelObservabilityPersistence } = await import("../lib/llm/model-observability-persistence.ts");
    const handle2 = installModelObservabilityPersistence({
      lingxiHome: harness.lingxiHome,
      policy: { enabled: true, persistTraceMetadata: true, persistPayloads: true, persistBlobs: true },
    });
    const onStart = Date.now();
    for (let i = 0; i < 100; i++) {
      await placeCall("memory", "sess-perf");
    }
    const onMs = Date.now() - onStart;
    handle2.flushSync();
    await handle2.close();

    // 宽松数量级 guard（§一百零三）：本地 HTTP 主导耗时，observability 开销
    // 不应把 wall time 推高 5 倍以上（正常应为同一量级）。
    expect(onMs).toBeLessThan(Math.max(offMs * 5, 5_000));
  }, 120_000);
});
