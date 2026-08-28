/**
 * Output Budget Fact（输出预算事实）— 持久化 request header 借鉴实现。
 *
 * 借鉴 deepseek-harness 的 adapter-owned materialized defaults：在最终序列化
 * body 上解析「输出预算是谁定的」（user-explicit / system-explicit /
 * hana-chat-default / sdk-derived / absent），连同 composition 一起物化进
 * provider_request_prepared 的持久 details（model_attempts.safe_details_json）。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resolveOutputBudgetFact } from "../core/provider-compat/output-budget.ts";
import { normalizeProviderPayload } from "../core/provider-compat.ts";
import { callText } from "../core/llm-client.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";

const DEEPSEEK_MODEL = {
  id: "deepseek-v4-pro",
  provider: "deepseek",
  api: "openai-completions",
  reasoning: true,
  maxTokens: 384000,
};

const ANTHROPIC_MODEL = {
  id: "claude-opus-4-7",
  provider: "anthropic",
  api: "anthropic-messages",
  reasoning: true,
  maxTokens: 128000,
};

describe("resolveOutputBudgetFact — ownership 分类", () => {
  it("separate 模型命中 Hana 聊天默认值 → hana-chat-default", () => {
    const fact = resolveOutputBudgetFact({
      model: "deepseek-v4-pro",
      messages: [{ role: "user", content: "hi" }],
      max_tokens: 65536,
    }, DEEPSEEK_MODEL);
    expect(fact).toEqual({
      field: "max_tokens",
      value: 65536,
      composition: "separate",
      ownership: "hana-chat-default",
      chatDefault: 65536,
      declaredMaxOutput: 384000,
    });
  });

  it("included 协议家族命中含思考余量的默认值 → hana-chat-default", () => {
    const fact = resolveOutputBudgetFact({
      model: "claude-opus-4-7",
      messages: [{ role: "user", content: "hi" }],
      max_tokens: 81920,
    }, ANTHROPIC_MODEL);
    expect(fact).toMatchObject({
      field: "max_tokens",
      value: 81920,
      composition: "included",
      ownership: "hana-chat-default",
      chatDefault: 81920,
    });
  });

  it("不等于默认值的未指定来源值 → sdk-derived（SDK 按剩余窗口收紧）", () => {
    const fact = resolveOutputBudgetFact({
      messages: [{ role: "user", content: "hi" }],
      max_completion_tokens: 20000,
    }, { id: "m", provider: "openai-compatible", api: "openai-completions", maxTokens: 262144 });
    expect(fact).toMatchObject({
      field: "max_completion_tokens",
      value: 20000,
      ownership: "sdk-derived",
    });
  });

  it("user/system 来源显式标记 → user-explicit / system-explicit，且记录声明上限", () => {
    const payload = { messages: [], max_tokens: 120000 };
    expect(resolveOutputBudgetFact(payload, DEEPSEEK_MODEL, { outputBudgetSource: "user" }))
      .toMatchObject({ ownership: "user-explicit", declaredMaxOutput: 384000 });
    expect(resolveOutputBudgetFact(
      { messages: [], max_tokens: 32000 },
      DEEPSEEK_MODEL,
      { mode: "utility", outputBudgetSource: "system" },
    )).toMatchObject({ ownership: "system-explicit", value: 32000 });
  });

  it("无 cap 字段但有 prompt → ownership=absent；两者皆无 → null", () => {
    const absent = resolveOutputBudgetFact({
      model: "m",
      messages: [{ role: "user", content: "hi" }],
    }, { id: "m", provider: "p", api: "openai-completions" });
    expect(absent).toMatchObject({ field: null, value: null, ownership: "absent" });

    // gemini 用 contents 承载 prompt
    expect(resolveOutputBudgetFact(
      { contents: [{ role: "user", parts: [{ text: "hi" }] }] },
      { id: "g", provider: "gemini", api: "google-generative-ai" },
    )).toMatchObject({ ownership: "absent" });

    expect(resolveOutputBudgetFact({ model: "m" }, { id: "m", provider: "p" })).toBeNull();
    expect(resolveOutputBudgetFact(null, DEEPSEEK_MODEL)).toBeNull();
  });
});

describe("resolveOutputBudgetFact × normalizeProviderPayload 最终 body", () => {
  it("chat 默认链路的最终 payload 解析出 fact（协议家族分别验证）", () => {
    const deepseekFinal = normalizeProviderPayload({
      model: "deepseek-v4-pro",
      messages: [{ role: "user", content: "hi" }],
    }, DEEPSEEK_MODEL, { mode: "chat" });
    expect(resolveOutputBudgetFact(deepseekFinal, DEEPSEEK_MODEL)).toMatchObject({
      field: "max_tokens",
      value: 65536,
      composition: "separate",
      ownership: "hana-chat-default",
    });

    const anthropicFinal = normalizeProviderPayload({
      model: "claude-opus-4-7",
      messages: [{ role: "user", content: "hi" }],
    }, ANTHROPIC_MODEL, { mode: "chat" });
    expect(resolveOutputBudgetFact(anthropicFinal, ANTHROPIC_MODEL)).toMatchObject({
      composition: "included",
      ownership: "hana-chat-default",
    });
  });

  it("协议必填兜底合成的模型上限也归 hana-chat-default 而非 sdk-derived", () => {
    // anthropic-messages 属 required capability：无预算时兜底声明值。
    const final = normalizeProviderPayload({
      model: "claude-opus-4-7",
      messages: [{ role: "user", content: "hi" }],
    }, ANTHROPIC_MODEL, { mode: "utility" });
    const fact = resolveOutputBudgetFact(final, ANTHROPIC_MODEL, { mode: "utility" });
    expect(fact).toMatchObject({
      field: "max_tokens",
      value: 128000,
      ownership: "hana-chat-default",
    });
  });
});

describe("callText × prepared details.outputBudget（持久化接线）", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;

  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
  });
  afterEach(() => {
    setModelCallObserver(null);
    vi.unstubAllGlobals();
  });

  function okFetch() {
    return vi.fn(async () => new Response(JSON.stringify({
      choices: [{ message: { content: "Title" }, finish_reason: "stop" }],
      usage: { prompt_tokens: 10, completion_tokens: 3, total_tokens: 13 },
    }), { status: 200 }));
  }

  const baseOptions = (extra: Record<string, unknown> = {}) => ({
    api: "openai-completions",
    baseUrl: "https://example.test/v1",
    model: DEEPSEEK_MODEL,
    systemPrompt: "sys",
    messages: [{ role: "user", content: "hi" }],
    usageContext: {
      source: { subsystem: "utility", operation: "title", surface: "system", trigger: "tool" },
    },
    ...extra,
  } as any);

  it("显式任务预算 → system-explicit 物化进 prepared details", async () => {
    vi.stubGlobal("fetch", okFetch());
    await callText(baseOptions({ maxTokens: 32000, outputPolicy: "bounded", outputBudgetSource: "system" }));

    const prepared = observer.eventsOfType("provider_request_prepared")[0];
    expect(prepared.details.outputBudget).toMatchObject({
      field: "max_tokens",
      value: 32000,
      composition: "separate",
      ownership: "system-explicit",
      declaredMaxOutput: 384000,
    });
  });

  it("无预算任务调用 → ownership=absent 也如实落盘", async () => {
    vi.stubGlobal("fetch", okFetch());
    await callText(baseOptions());

    const prepared = observer.eventsOfType("provider_request_prepared")[0];
    expect(prepared.details.outputBudget).toMatchObject({
      field: null,
      value: null,
      ownership: "absent",
    });
  });
});
