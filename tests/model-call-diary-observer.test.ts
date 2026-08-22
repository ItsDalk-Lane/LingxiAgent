/**
 * MC-10 diary temporary summary × ModelCallObserver — 旁路闭合的运行时证明。
 *
 * Phase 3.5 确认的生产可达旁路（MODEL_CALL_CLOSURE_DELTA.md）：
 *   /diary → generateDiaryCompactionSummary → Pi generateSummary（无 streamFn）
 *   → completeSimple → Provider
 * 原先不经 streamFunction/callText/任何 observer。本测试经**真实** Pi
 * completeSimple 链（stub 全局 fetch 伪 Provider 响应）证明：
 *   1. 该路径现在有完整 logical call 生命周期（logical_call_start → attempt_start
 *      (logical_boundary) → semantic_response_completed → logical_call_end）；
 *   2. 不伪造 provider_request_prepared / provider_response_received（无 hook
 *      是事实，§五）；
 *   3. ledger 记账 + metadata.{modelCallId, traceId, parentCallId} 关联；
 *   4. 失败路径：Provider 错误 → attempt/logical 错误终态 + ledger recordError；
 *   5. 同一 diary trace 内多次临时摘要 parentCallId=null（§四十七）。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { generateDiaryCompactionSummary } from "../lib/diary/diary-writer.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";
import { runWithNewModelTrace } from "../lib/llm/model-trace-scope.ts";
import { createUsageLedger } from "../lib/llm/usage-ledger.ts";

const MODEL = {
  id: "test-model",
  provider: "test-provider",
  api: "openai-completions",
  baseUrl: "https://example.test/v1",
  maxTokens: 8192,
  input: ["text"],
  // pi-ai calculateCost 读取 model.cost.tiers——真实 models.json 条目必带。
  cost: { input: 1, output: 2, cacheRead: 0.1, cacheWrite: 1.25, total: 0 },
};

/**
 * Pi openai adapter 默认请求流式（stream:true）——伪 Provider 必须回 SSE。
 * chunk 形状：delta.content 增量 + 末 chunk finish_reason + [DONE]。
 */
function completionsOkFetch(content = "summary text") {
  const sseBody = [
    `data: ${JSON.stringify({ id: "chatcmpl-1", object: "chat.completion.chunk", created: 0, model: "test-model", choices: [{ index: 0, delta: { role: "assistant", content }, finish_reason: null }] })}`,
    "",
    `data: ${JSON.stringify({ id: "chatcmpl-1", object: "chat.completion.chunk", created: 0, model: "test-model", choices: [{ index: 0, delta: {}, finish_reason: "stop" }], usage: { prompt_tokens: 10, completion_tokens: 4, total_tokens: 14 } })}`,
    "",
    "data: [DONE]",
    "",
  ].join("\n");
  return vi.fn(async () => new Response(sseBody, {
    status: 200,
    headers: { "content-type": "text/event-stream" },
  }));
}

let observer: ReturnType<typeof createTestModelCallObserver>;

beforeEach(() => {
  observer = createTestModelCallObserver();
  setModelCallObserver(observer);
});
afterEach(() => {
  setModelCallObserver(null);
  vi.unstubAllGlobals();
});

async function flushTerminal() {
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
}

describe("MC-10 diary temporary summary — 旁路闭合", () => {
  it("真实 Pi generateSummary 链：完整生命周期 + ledger 关联 + 不伪造 wire 事件", async () => {
    vi.stubGlobal("fetch", completionsOkFetch("今日素材摘要"));
    const ledger = createUsageLedger({});

    const summary = await runWithNewModelTrace({ origin: "diary" }, () =>
      generateDiaryCompactionSummary({
        messages: [{ role: "user", content: [{ type: "text", text: "hello" }], timestamp: Date.now() }] as any,
        model: MODEL as any,
        apiKey: "test-key",
        headers: undefined,
        previousSummary: "",
        usageLedger: ledger,
        agentId: "agent-1",
      }),
    );
    await flushTerminal();

    expect(summary).toContain("今日素材摘要");

    const starts = observer.eventsOfType("logical_call_start");
    expect(starts).toHaveLength(1);
    const callId = starts[0].callId;
    expect(callId).toMatch(/^mc_/);
    expect(starts[0].source).toMatchObject({
      subsystem: "memory",
      operation: "diary_temporary_summary",
      surface: "background",
      trigger: "system",
    });
    expect(starts[0].details).toMatchObject({ path: "pi_direct_summary", traceOrigin: "diary" });
    // 无 streamFn 的 direct summary：logical_boundary attempt、零 wire 事件
    expect(observer.eventsForCall(callId).map((event) => event.eventType)).toEqual([
      "logical_call_start",
      "attempt_start",
      "semantic_response_completed",
      "logical_call_end",
    ]);
    expect(observer.attemptsForCall(callId)[0]).toMatch(/^ma_/);
    const attemptStart = observer.eventsOfType("attempt_start")[0];
    expect(attemptStart.details).toMatchObject({ attemptVisibility: "logical_boundary" });

    // ledger：一条 usage_missing 之外的正常记录 + 三元组关联
    const { entries } = ledger.list({});
    expect(entries.length).toBe(1);
    expect(entries[0].metadata).toMatchObject({
      modelCallId: callId,
      traceId: starts[0].traceId,
      parentCallId: null,
    });
    observer.assertTraceGraphValid();
  });

  it("同一 diary trace 的两次临时摘要：same traceId、parent 均 null（§四十七）", async () => {
    vi.stubGlobal("fetch", completionsOkFetch("素材"));
    const ledger = createUsageLedger({});

    await runWithNewModelTrace({ origin: "diary" }, async () => {
      await generateDiaryCompactionSummary({
        messages: [{ role: "user", content: [{ type: "text", text: "a" }], timestamp: 1 }] as any,
        model: MODEL as any, apiKey: "k", headers: undefined, usageLedger: ledger, agentId: "agent-1",
      });
      await generateDiaryCompactionSummary({
        messages: [{ role: "user", content: [{ type: "text", text: "b" }], timestamp: 2 }] as any,
        model: MODEL as any, apiKey: "k", headers: undefined, usageLedger: ledger, agentId: "agent-1",
      });
    });
    await flushTerminal();

    const calls = observer.callIds();
    expect(calls).toHaveLength(2);
    const identityA = observer.callIdentity(calls[0])!;
    const identityB = observer.callIdentity(calls[1])!;
    expect(identityA.traceId).toBe(identityB.traceId);
    expect(identityA.parentCallId).toBeNull();
    expect(identityB.parentCallId).toBeNull();
    observer.assertTraceGraphValid();
  });

  it("Provider 错误：错误终态 + 业务异常原样抛出 + ledger recordError", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(
      JSON.stringify({ error: { message: "TOPSECRET_PROVIDER_ERROR", type: "rate_limit_error" } }),
      { status: 429, headers: { "content-type": "application/json" } },
    )));
    const ledger = createUsageLedger({});

    await expect(runWithNewModelTrace({ origin: "diary" }, () =>
      generateDiaryCompactionSummary({
        messages: [{ role: "user", content: [{ type: "text", text: "x" }], timestamp: 1 }] as any,
        model: MODEL as any, apiKey: "k", headers: undefined, usageLedger: ledger, agentId: "agent-1",
      }),
    )).rejects.toThrow();
    await flushTerminal();

    const events = observer.events;
    const callEvents = events.filter((event) => event.callId === observer.callIds()[0]);
    expect(callEvents.map((event) => event.eventType)).toEqual([
      "logical_call_start",
      "attempt_start",
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
    expect(observer.eventsOfType("logical_call_end")[0].status).toBe("error");
    // Provider 错误正文绝不进事件（safe error contract）
    observer.assertNoSensitiveContent(["TOPSECRET_PROVIDER_ERROR"]);
    const { entries } = ledger.list({});
    expect(entries.length).toBe(1);
    expect(entries[0].status).toBe("error");
    expect(entries[0].metadata.modelCallId).toBe(observer.callIds()[0]);
  });

  it("空消息列表短路：不产生模型调用（保持原行为）", async () => {
    const fetchSpy = completionsOkFetch();
    vi.stubGlobal("fetch", fetchSpy);
    const result = await generateDiaryCompactionSummary({
      messages: [],
      model: MODEL as any,
      apiKey: "k",
      headers: undefined,
    });
    expect(result).toBe("");
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(observer.events).toHaveLength(0);
  });
});
