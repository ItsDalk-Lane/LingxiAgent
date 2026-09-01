import { describe, expect, it } from "vitest";

import {
  MODEL_VIDEO_TRANSPORTS,
  modelSupportsDirectVideoInput,
  modelSupportsVideoInput,
  modelSupportsVideoMimeType,
  resolveModelVideoInputTransport,
} from "../shared/model-capabilities.ts";

describe("model video capability transport", () => {
  it("keeps semantic video capability separate from provider transport support", () => {
    const model = {
      id: "kimi-for-coding",
      provider: "kimi-coding",
      api: "anthropic-messages",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
    };

    expect(modelSupportsVideoInput(model)).toBe(true);
    expect(resolveModelVideoInputTransport(model)).toBe(MODEL_VIDEO_TRANSPORTS.UNSUPPORTED);
    expect(modelSupportsDirectVideoInput(model)).toBe(false);
  });

  it("allows native Gemini video through inlineData transport", () => {
    const model = {
      id: "gemini-3-flash-preview",
      provider: "gemini",
      api: "google-generative-ai",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
      baseUrl: "https://generativelanguage.googleapis.com/v1beta",
    };

    expect(resolveModelVideoInputTransport(model)).toBe(MODEL_VIDEO_TRANSPORTS.GEMINI_INLINE_DATA);
    expect(modelSupportsDirectVideoInput(model)).toBe(true);
  });

  it("allows high-confidence OpenAI-compatible video_url providers only", () => {
    expect(resolveModelVideoInputTransport({
      id: "qwen3-vl-plus",
      provider: "dashscope",
      api: "openai-completions",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    })).toBe(MODEL_VIDEO_TRANSPORTS.OPENAI_VIDEO_URL);

    expect(resolveModelVideoInputTransport({
      id: "kimi-k2.6",
      provider: "moonshot",
      api: "openai-completions",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
      baseUrl: "https://api.moonshot.cn/v1",
    })).toBe(MODEL_VIDEO_TRANSPORTS.OPENAI_VIDEO_URL);
  });

  it("allows official MiMo video models through OpenAI video_url transport", () => {
    const model = {
      id: "mimo-v2.5",
      provider: "mimo",
      api: "openai-completions",
      input: ["text", "image"],
      video: true,
      baseUrl: "https://api.xiaomimimo.com/v1",
    };

    expect(modelSupportsVideoInput(model)).toBe(true);
    expect(resolveModelVideoInputTransport(model)).toBe(MODEL_VIDEO_TRANSPORTS.OPENAI_VIDEO_URL);
    expect(modelSupportsDirectVideoInput(model)).toBe(true);
  });

  it("routes user-declared video through the generic transport for unverified providers", () => {
    // 模型声明（勾选/词典 video）即授权：未知供应商 + OpenAI 兼容线协议 → 通用档放行。
    expect(resolveModelVideoInputTransport({
      id: "custom-video",
      provider: "custom",
      api: "openai-completions",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
      baseUrl: "https://api.example.com/v1",
    })).toBe(MODEL_VIDEO_TRANSPORTS.GENERIC_OPENAI_VIDEO_URL);
    expect(modelSupportsDirectVideoInput({
      id: "custom-video",
      provider: "custom",
      api: "openai-completions",
      compat: { hanaVideoInput: true },
    })).toBe(true);

    // 未声明视频输入：无论供应商是谁都判 NONE——声明是放行的前提。
    expect(resolveModelVideoInputTransport({
      id: "dashscope-plain",
      provider: "dashscope",
      api: "openai-completions",
      input: ["text", "image"],
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    })).toBe(MODEL_VIDEO_TRANSPORTS.NONE);

    // 非视频内容位的线协议（anthropic/responses）没有通用档，声明了也拦。
    expect(resolveModelVideoInputTransport({
      id: "custom-video",
      provider: "custom",
      api: "anthropic-messages",
      compat: { hanaVideoInput: true },
      baseUrl: "https://api.example.com/v1",
    })).toBe(MODEL_VIDEO_TRANSPORTS.UNSUPPORTED);
  });

  it("enforces the verified provider and format intersection before send", () => {
    const model = (provider: string, api = "openai-completions") => ({
      id: `${provider}-video`,
      provider,
      api,
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
    });
    const formats = ["video/mp4", "video/quicktime", "video/webm"];

    expect(formats.map(mime => modelSupportsVideoMimeType(model("gemini", "google-generative-ai"), mime)))
      .toEqual([true, true, true]);
    expect(formats.map(mime => modelSupportsVideoMimeType(model("dashscope"), mime)))
      .toEqual([true, true, false]);
    expect(formats.map(mime => modelSupportsVideoMimeType(model("moonshot"), mime)))
      .toEqual([true, false, false]);
    expect(formats.map(mime => modelSupportsVideoMimeType(model("mimo"), mime)))
      .toEqual([true, true, false]);
    // 通用档不按端点收窄：格式交由供应商显式裁决（4xx 可见）。
    expect(formats.map(mime => modelSupportsVideoMimeType(model("custom"), mime)))
      .toEqual([true, true, true]);
  });

  it("allows qwen TokenPlan subscription variants through OpenAI video_url transport", () => {
    const tokenPlanModel = (provider: string) => ({
      id: "qwen3.6-flash",
      provider,
      api: "openai-completions",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
      baseUrl: "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
    });

    for (const provider of ["qwen-token-plan", "qwen-token-plan-cn", "qwen-token-plan-individual"]) {
      expect(resolveModelVideoInputTransport(tokenPlanModel(provider)))
        .toBe(MODEL_VIDEO_TRANSPORTS.OPENAI_VIDEO_URL);
    }
  });

  it("recognizes the MaaS domain even when the provider id is unknown", () => {
    expect(resolveModelVideoInputTransport({
      id: "qwen3.6-flash",
      provider: "my-custom-provider",
      api: "openai-completions",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
      baseUrl: "https://token-plan.cn-hangzhou.maas.aliyuncs.com/compatible-mode/v1",
    })).toBe(MODEL_VIDEO_TRANSPORTS.OPENAI_VIDEO_URL);

    // MaaS 域名只放行 openai-completions 线协议；其他 api 家族仍走 UNSUPPORTED。
    expect(resolveModelVideoInputTransport({
      id: "qwen3.6-flash",
      provider: "my-custom-provider",
      api: "anthropic-messages",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
      baseUrl: "https://token-plan.cn-hangzhou.maas.aliyuncs.com/compatible-mode/v1",
    })).toBe(MODEL_VIDEO_TRANSPORTS.UNSUPPORTED);
  });

  it("extends the DashScope format intersection to TokenPlan variants", () => {
    const formats = ["video/mp4", "video/quicktime", "video/webm"];
    expect(formats.map(mime => modelSupportsVideoMimeType({
      id: "qwen3.6-flash",
      provider: "qwen-token-plan-cn",
      api: "openai-completions",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
    }, mime))).toEqual([true, true, false]);
  });

  it("recognizes Kimi brand endpoints alongside legacy Moonshot domains", () => {
    expect(resolveModelVideoInputTransport({
      id: "kimi-k2.6",
      provider: "kimi-coding",
      api: "openai-completions",
      input: ["text", "image"],
      compat: { hanaVideoInput: true },
      baseUrl: "https://api.kimi.com/coding/v1",
    })).toBe(MODEL_VIDEO_TRANSPORTS.OPENAI_VIDEO_URL);

    for (const provider of ["moonshotai", "moonshotai-cn"]) {
      expect(resolveModelVideoInputTransport({
        id: "kimi-k2.6",
        provider,
        api: "openai-completions",
        input: ["text", "image"],
        compat: { hanaVideoInput: true },
      })).toBe(MODEL_VIDEO_TRANSPORTS.OPENAI_VIDEO_URL);
    }

    expect(modelSupportsVideoMimeType({
      id: "kimi-k2.6",
      provider: "kimi-coding",
      api: "openai-completions",
      compat: { hanaVideoInput: true },
      baseUrl: "https://api.kimi.com/coding/v1",
    }, "video/webm")).toBe(false);
  });
});
