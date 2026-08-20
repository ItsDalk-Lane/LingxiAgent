import { describe, expect, it, vi } from "vitest";
import { ModelManager } from "../core/model-manager.ts";

describe("ModelManager static credential boundary", () => {
  it("同步校验只返回模型状态，明文读取只能走专用窄接口", () => {
    const manager = new ModelManager({ lingxiHome: "/tmp/lingxi-static-boundary-test" }) as any;
    const model = {
      id: "vision-model",
      provider: "example",
      api: "openai-completions",
      baseUrl: "https://example.test/v1",
      input: ["text", "image"],
    };
    manager._availableModels = [model];
    manager.providerRegistry = {
      getAllProvidersRaw: vi.fn(() => ({
        example: { api_key: "saved-key" },
      })),
    };

    expect(manager.resolveModelForValidation({ id: "vision-model", provider: "example" })).toBe(model);
    expect(JSON.stringify(manager.resolveModelForValidation(model))).not.toContain("saved-key");
    expect(manager.readSavedProviderApiKey("example")).toBe("saved-key");
    expect(manager.resolveProviderCredentials).toBeUndefined();
    expect(manager.resolveModelWithCredentials).toBeUndefined();
    expect(manager.executionRouter).toBeUndefined();
  });
});
