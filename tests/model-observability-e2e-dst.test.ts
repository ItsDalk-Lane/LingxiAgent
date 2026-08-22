/**
 * Phase 10 DST 专项（S30，任务书 §八十/§八十一）。
 *
 * 当前 date bucket 契约 = 固定 utcOffsetMinutes（SQL strftime）。对**历史**跨
 * DST 窗口：renderer 发送「当前 offset」会把历史 call 分到错误日期。
 *
 * 本测试先用 failing test 证明问题存在（期望 IANA timeZone bucket 语义），
 * 修复后本文件全绿；非 DST 时区反向回归（§一百五十八）。
 *
 * 场景（America/Los_Angeles，DST start 2026-03-08 02:00）：
 *   A = 2026-03-07T23:30:00-08:00（PST）→ 当地日期 2026-03-07
 *   B = 2026-03-08T23:30:00-07:00（PDT）→ 当地日期 2026-03-08
 * 固定 offset（无论 -420 还是 -480）都无法同时把 A/B 分到正确日期。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { callText } from "../core/llm-client.ts";
import { normalizeModelObservabilityAggregateQuery } from "../lib/llm/model-observability-query-types.ts";
import {
  createScenarioHarness,
  flushAsync,
  openaiCompletionsJson,
  type ScenarioHarness,
} from "./helpers/model-observability-scenario-harness.ts";

const LA = "America/Los_Angeles";
const CALL_A_UTC = "2026-03-08T07:30:00.000Z"; // 当地 2026-03-07 23:30 PST
const CALL_B_UTC = "2026-03-09T06:30:00.000Z"; // 当地 2026-03-08 23:30 PDT

let harness: ScenarioHarness;

beforeEach(async () => {
  harness = await createScenarioHarness();
});
afterEach(async () => {
  viUseRealDate();
  await harness.close();
  harness.cleanup();
});

/** 只伪造 Date（保持 setImmediate/网络定时器真实，witness HTTP 不受影响）。 */
function viSetDate(iso: string) {
  vi.useFakeTimers({ now: new Date(iso).getTime(), toFake: ["Date"] });
}
function viUseRealDate() {
  vi.useRealTimers();
}

async function placeCall(content: string) {
  harness.witness.scriptNext({ kind: "json", body: openaiCompletionsJson({ content }) });
  await callText({
    api: "openai-completions",
    apiKey: "sk-dst-e2e",
    baseUrl: harness.witness.baseUrl,
    model: { id: "witness-model", provider: "witness-provider" } as any,
    systemPrompt: "S",
    messages: [{ role: "user", content }],
    usageContext: {
      source: { subsystem: "memory", operation: "dst_e2e", surface: "desktop", trigger: "user" },
      attribution: { kind: "agent", agentId: "agent-e2e" },
    },
  } as any);
  await flushAsync(4);
}

describe("E2E truth — DST / date bucket（S30）", () => {
  it("America/Los_Angeles 跨 DST：IANA timeZone bucket 把 A→03-07、B→03-08（修复目标）", async () => {
    viSetDate(CALL_A_UTC);
    await placeCall("E2E_DST_CALL_A");
    viSetDate(CALL_B_UTC);
    await placeCall("E2E_DST_CALL_B");
    viUseRealDate();
    harness.flush();
    await flushAsync(3);

    // 新契约：dateBucket.timeZone（IANA）——当前实现无此字段 → 本测试先失败
    const normalized = normalizeModelObservabilityAggregateQuery({
      groupBy: ["date"],
      dateBucket: { bucket: "day", timeZone: LA },
    });
    expect(normalized.ok).toBe(true);
    if (normalized.ok === false) throw new Error(normalized.error.message);

    const aggregate = harness.query().queryAggregate(normalized.value);
    expect(aggregate.ok).toBe(true);
    if (!aggregate.ok) throw new Error((aggregate as any).error?.message ?? "aggregate failed");
    const dateBuckets = aggregate.value.groups.map((group: any) => group.values.date).sort();
    // 真相：A 当地 03-07，B 当地 03-08（固定 offset 会给出 03-08/03-07 互换）
    expect(dateBuckets).toEqual(["2026-03-07", "2026-03-08"]);
  });

  it("固定 utcOffsetMinutes 语义保持（非 DST 时区反向回归）", async () => {
    viSetDate("2026-06-01T15:30:00.000Z"); // UTC+8 → 当地 2026-06-01 23:30
    await placeCall("E2E_DST_CALL_C");
    viUseRealDate();
    harness.flush();
    await flushAsync(3);

    const normalized = normalizeModelObservabilityAggregateQuery({
      groupBy: ["date"],
      dateBucket: { bucket: "day", utcOffsetMinutes: 480 },
    });
    expect(normalized.ok).toBe(true);
    if (normalized.ok === false) throw new Error(normalized.error.message);
    const aggregate = harness.query().queryAggregate(normalized.value);
    expect(aggregate.ok).toBe(true);
    if (!aggregate.ok) throw new Error("aggregate failed");
    expect(aggregate.value.groups.map((group: any) => group.values.date)).toEqual(["2026-06-01"]);
  });
});
