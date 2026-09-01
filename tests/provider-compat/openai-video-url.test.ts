import { describe, expect, it } from "vitest";

import { normalizeProviderPayload } from "../../core/provider-compat.ts";

describe("provider-compat/openai-video-url", () => {
  it("converts Moonshot Kimi data:video image_url blocks to video_url", () => {
    const payload = {
      model: "kimi-k2.6",
      messages: [{
        role: "user",
        content: [
          { type: "image_url", image_url: { url: "data:video/mp4;base64,AAAA" } },
          { type: "text", text: "看一下" },
        ],
      }],
    };

    const result = normalizeProviderPayload(payload, {
      id: "kimi-k2.6",
      provider: "moonshot",
      api: "openai-completions",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
      baseUrl: "https://api.moonshot.cn/v1",
    }, { mode: "chat" });

    expect(result).not.toBe(payload);
    expect(result.messages[0].content[0]).toEqual({
      type: "video_url",
      video_url: { url: "data:video/mp4;base64,AAAA" },
    });
    expect(payload.messages[0].content[0].type).toBe("image_url");
  });

  it("also accepts SDK/client camelCase imageUrl blocks", () => {
    const payload = {
      model: "qwen3-vl-plus",
      messages: [{
        role: "user",
        content: [
          { type: "image_url", imageUrl: { url: "data:video/webm;base64,BBBB" } },
        ],
      }],
    };

    const result = normalizeProviderPayload(payload, {
      id: "qwen3-vl-plus",
      provider: "dashscope",
      api: "openai-completions",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    }, { mode: "chat" });

    expect(result.messages[0].content[0]).toEqual({
      type: "video_url",
      video_url: { url: "data:video/webm;base64,BBBB" },
    });
  });
  it("rewrites video blocks even when a provider module shadows the old registry slot (GLM 400 regression)", () => {
    // zhipu/kimi 等自有模块在 first-match-wins 链上位于 openaiVideoUrl 之前，
    // 视频改写已上移中心层——带 thinkingFormat 的真实模型形状必须仍然改写成功。
    const payload = {
      model: "glm-5.3-flash",
      messages: [{
        role: "user",
        content: [
          { type: "text", text: "视频内容是什么？" },
          { type: "image_url", image_url: { url: "data:video/mp4;base64,QUJD" } },
        ],
      }],
    };
    const glmModel = {
      id: "glm-5.3-flash",
      provider: "zhipu-coding",
      api: "openai-completions",
      input: ["text", "image"],
      compat: {
        hanaVideoInput: true,
        thinkingFormat: "zhipu",
        supportsStore: false,
        supportsDeveloperRole: false,
        supportsReasoningEffort: false,
      },
      baseUrl: "https://api.z.ai/api/coding/paas/v4",
    };

    const result = normalizeProviderPayload(payload, glmModel, { mode: "chat" });

    expect(result.messages[0].content[1]).toEqual({
      type: "video_url",
      video_url: { url: "data:video/mp4;base64,QUJD" },
    });
    // zhipu 模块自身的 thinking 协议处理不受中心化改写影响。
    expect(result.model).toBe("glm-5.3-flash");
  });

  it("rewrites video blocks for kimi models carrying the kimi thinkingFormat", () => {
    const payload = {
      model: "kimi-k2.6",
      messages: [{
        role: "user",
        content: [
          { type: "image_url", image_url: { url: "data:video/mp4;base64,WERG" } },
        ],
      }],
    };
    const kimiModel = {
      id: "kimi-k2.6",
      provider: "moonshot",
      api: "openai-completions",
      input: ["text", "image"],
      compat: { hanaVideoInput: true, thinkingFormat: "kimi" },
      baseUrl: "https://api.moonshot.cn/v1",
    };

    const result = normalizeProviderPayload(payload, kimiModel, { mode: "chat" });

    expect(result.messages[0].content[0].type).toBe("video_url");
  });
});
