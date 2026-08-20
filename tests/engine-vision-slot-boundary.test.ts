import { describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";

describe("Engine vision Slot boundary", () => {
  it("视觉执行只采用标准 vision Slot 的实时解析结果", async () => {
    const execution = {
      model: { id: "vision-fallback", provider: "chat-provider", input: ["text", "image"] },
      provider: "chat-provider",
      api: "openai-completions",
      apiKey: "fresh-key",
      baseUrl: "https://example.test/v1",
      headers: {},
    };
    const resolveAuxiliaryExecution = vi.fn(async () => execution);
    const target = {
      isVisionAuxiliaryEnabled: () => true,
      resolveAuxiliaryExecution,
      getSharedModels: vi.fn(() => null),
      resolveModelWithCredentialsFresh: vi.fn(),
    };

    await expect(LingxiEngine.prototype.resolveVisionConfigFresh.call(target)).resolves.toBe(execution);
    expect(resolveAuxiliaryExecution).toHaveBeenCalledWith("vision");
    expect(target.getSharedModels).not.toHaveBeenCalled();
    expect(target.resolveModelWithCredentialsFresh).not.toHaveBeenCalled();
  });

  it("视觉辅助关闭时不解析任何模型", async () => {
    const resolveAuxiliaryExecution = vi.fn();
    const target = {
      isVisionAuxiliaryEnabled: () => false,
      resolveAuxiliaryExecution,
    };

    await expect(LingxiEngine.prototype.resolveVisionConfigFresh.call(target)).resolves.toBeNull();
    expect(resolveAuxiliaryExecution).not.toHaveBeenCalled();
  });
});
