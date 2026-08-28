/**
 * 「最大输出是否包含思维链」契约测试
 *
 * 语义前提（不能统一计算）：
 *   - included：思维链消耗计入 max output 上限（Anthropic Messages / Gemini /
 *     OpenAI Responses 等协议保证，或目录/用户显式声明）。
 *   - separate：声明的 max output 只约束最终回答，思维链由服务端独立预算
 *     （DeepSeek 官方 reasoner 家族等）。
 */

import { describe, expect, it } from "vitest";
import {
  getOutputThinkingComposition,
} from "../shared/model-capabilities.ts";
import { resolveOutputBudgetPolicy } from "../core/provider-compat/output-budget.ts";
import { validateProviderModels } from "../shared/provider-model-validation.ts";

describe("getOutputThinkingComposition", () => {
  it("compat 显式声明优先于一切推导", () => {
    // 即使线协议是「计入」家族，显式 false 也以声明为准。
    const anthropicOverride = getOutputThinkingComposition({
      provider: "custom-proxy",
      api: "anthropic-messages",
      compat: { outputIncludesThinking: false },
    });
    expect(anthropicOverride).toBe("separate");

    const openaiOverride = getOutputThinkingComposition({
      provider: "volcengine",
      api: "openai-completions",
      reasoning: true,
      compat: { outputIncludesThinking: true },
    });
    expect(openaiOverride).toBe("included");
  });

  it("顶层 outputIncludesThinking 是次级真理源（model-sync 投影位）", () => {
    expect(getOutputThinkingComposition({
      provider: "deepseek",
      api: "openai-completions",
      reasoning: true,
      outputIncludesThinking: true,
    })).toBe("included");
    expect(getOutputThinkingComposition({
      provider: "any-provider",
      api: "",
      outputIncludesThinking: false,
    })).toBe("separate");
    // 显式 false 压过协议家族的 included 兜底
    expect(getOutputThinkingComposition({
      provider: "anthropic",
      api: "anthropic-messages",
      outputIncludesThinking: false,
    })).toBe("separate");
  });

  it("语义由协议保证的线协议家族按 included 兜底", () => {
    for (const api of [
      "anthropic-messages",
      "google-generative-ai",
      "google-vertex",
      "openai-responses",
      "openai-codex-responses",
      "azure-openai-responses",
    ]) {
      expect(getOutputThinkingComposition({ id: "m", provider: "p", api, reasoning: true })).toBe("included");
    }
  });

  it("OpenAI ChatCompletions 推理模型不统一视为计入，默认 separate", () => {
    // 正是用户反馈的错口径：这里历史上把思考与输出混在一起算。
    expect(getOutputThinkingComposition({
      provider: "deepseek",
      api: "openai-completions",
      reasoning: true,
      maxTokens: 64000,
    })).toBe("separate");
    expect(getOutputThinkingComposition({ id: "unknown-model", provider: "p" })).toBe("separate");
  });

  it("DeepSeek 官方 endpoint 不随线协议翻转：Responses / Anthropic 兼容通道仍按 separate", () => {
    // 官方语义是 CoT 独立预算，换成 Responses wire family 也一样；
    // 显式声明仍可覆盖这条基线。
    expect(getOutputThinkingComposition({
      id: "deepseek-v4-flash",
      provider: "deepseek-responses",
      api: "openai-responses",
      baseUrl: "https://api.deepseek.com",
      reasoning: true,
    })).toBe("separate");
    expect(getOutputThinkingComposition({
      id: "deepseek-v4-pro",
      provider: "custom-relay",
      api: "anthropic-messages",
      baseUrl: "https://api.deepseek.com",
      reasoning: true,
    })).toBe("separate");
    expect(getOutputThinkingComposition({
      id: "deepseek-v4-flash",
      provider: "deepseek-responses",
      api: "openai-responses",
      outputIncludesThinking: true,
    })).toBe("included");
  });
});

describe("resolveOutputBudgetPolicy — 按 composition 分流", () => {
  it("separate 模型维持历史 64K 纯答案封顶", () => {
    const policy = resolveOutputBudgetPolicy({
      id: "deepseek-v4-pro",
      provider: "deepseek",
      api: "openai-completions",
      reasoning: true,
      maxTokens: 384000,
      outputIncludesThinking: false,
    }, { mode: "chat", outputBudgetSource: "sdk-default" });

    expect(policy.thinkingSharesOutput).toBe(false);
    expect(policy.defaultMaxTokens).toBe(65536);
    expect(policy.modelLimit).toBe(384000);
  });

  it("included 模型的聊天默认值包含思考余量且不超过模型上限", () => {
    // 有目录上限：64K 答案目标 + 16K 思考余量，仍 ≤ 模型声明输出上限
    const bounded = resolveOutputBudgetPolicy({
      id: "thinking-relay",
      provider: "volcengine",
      api: "openai-completions",
      reasoning: true,
      maxTokens: 131072,
      outputIncludesThinking: true,
    }, { mode: "chat", outputBudgetSource: "sdk-default" });
    expect(bounded.thinkingSharesOutput).toBe(true);
    expect(bounded.defaultMaxTokens).toBe(81920);

    // 无声明上限：默认值同样给思考留余量
    const unbounded = resolveOutputBudgetPolicy({
      id: "claude",
      provider: "anthropic",
      api: "anthropic-messages",
      reasoning: true,
    }, { mode: "chat" });
    expect(unbounded.defaultMaxTokens).toBe(81920);

    // 上限低于默认和时不放大
    const small = resolveOutputBudgetPolicy({
      id: "mini-thinking",
      provider: "volcengine-coding",
      api: "openai-completions",
      reasoning: true,
      maxTokens: 32768,
      outputIncludesThinking: true,
    }, { mode: "chat", outputBudgetSource: "sdk-default" });
    expect(small.defaultMaxTokens).toBe(32768);
  });

  it("user/system 来源的上限不被聊天默认值降档（两种 composition 一致）", () => {
    for (const outputIncludesThinking of [true, false]) {
      const policy = resolveOutputBudgetPolicy({
        id: "user-model",
        provider: "custom",
        api: "openai-completions",
        maxTokens: 128000,
        outputIncludesThinking,
      }, { mode: "chat", outputBudgetSource: "user" });
      expect(policy.preserveForSource).toBe(true);
      expect(policy.applyChatDefault).toBe(false);
    }
  });
});

describe("provider-model-validation — outputIncludesThinking", () => {
  const base = { baseUrl: "https://example.com" };

  it("接受布尔值", () => {
    expect(() => validateProviderModels("custom", [
      { id: "a", outputIncludesThinking: true },
      { id: "b", outputIncludesThinking: false },
    ], base)).not.toThrow();
  });

  it("接受 null 显式清除覆盖，拒绝其他非法值", () => {
    expect(() => validateProviderModels("custom", [
      { id: "a", outputIncludesThinking: null },
    ], base)).not.toThrow();
    expect(() => validateProviderModels("custom", [
      { id: "a", outputIncludesThinking: "yes" },
    ], base)).toThrow(/expected a boolean or null/);
  });
});
