import { describe, expect, it } from "vitest";
import { normalizeProviderPayload } from "../../core/provider-compat.ts";

const ollamaModel = {
  id: "gemma4:12b-nvfp4",
  provider: "ollama",
  api: "openai-completions",
  baseUrl: "http://localhost:11434/v1",
};

describe("provider-compat/ollama", () => {
  it("matches ollama provider and leaves normal payloads untouched", () => {
    const payload = {
      model: "gemma4:12b-nvfp4",
      messages: [{ role: "user", content: "hello" }],
    };

    const result = normalizeProviderPayload(payload, ollamaModel, { mode: "chat" });

    // No response_format injected when no schema is provided
    expect(result).not.toHaveProperty("response_format");
    expect(result.model).toBe("gemma4:12b-nvfp4");
  });

  it("injects response_format.json_schema when responseSchema is provided", () => {
    const schema = {
      type: "object",
      properties: {
        name: { type: "string" },
        capital: { type: "string" },
      },
      required: ["name", "capital"],
    };
    const payload = {
      model: "gemma4:12b-nvfp4",
      messages: [{ role: "user", content: "Tell me about Canada." }],
    };

    const result = normalizeProviderPayload(payload, ollamaModel, {
      mode: "chat",
      responseSchema: schema,
    });

    expect(result.response_format).toEqual({
      type: "json_schema",
      json_schema: {
        name: "structured_output",
        schema,
        strict: true,
      },
    });
  });

  it("does not inject response_format when responseSchema is absent", () => {
    const payload = {
      model: "gemma4:12b-nvfp4",
      messages: [{ role: "user", content: "hello" }],
    };

    const result = normalizeProviderPayload(payload, ollamaModel, {
      mode: "chat",
    });

    expect(result).not.toHaveProperty("response_format");
  });

  it("does not match non-ollama providers", () => {
    const payload = {
      model: "some-model",
      messages: [{ role: "user", content: "hello" }],
    };
    const nonOllamaModel = { ...ollamaModel, provider: "openai" };

    const result = normalizeProviderPayload(payload, nonOllamaModel, {
      mode: "chat",
      responseSchema: { type: "object" },
    });

    // Non-ollama provider should not get response_format injected
    expect(result).not.toHaveProperty("response_format");
  });

  it("preserves existing payload fields when injecting response_format", () => {
    const payload = {
      model: "gemma4:12b-nvfp4",
      messages: [{ role: "user", content: "Describe" }],
      temperature: 0,
      max_tokens: 1024,
    };

    const result = normalizeProviderPayload(payload, ollamaModel, {
      mode: "chat",
      responseSchema: { type: "object", properties: { summary: { type: "string" } } },
    });

    expect(result.temperature).toBe(0);
    expect(result.max_tokens).toBe(1024);
    expect(result.response_format).toBeDefined();
  });

  describe("num_ctx bridging", () => {
    it("injects options.num_ctx from model.contextWindow", () => {
      const model = { ...ollamaModel, contextWindow: 262144 };
      const payload = {
        model: "gemma4:31b-nvfp4",
        messages: [{ role: "user", content: "hi" }],
      };

      const result = normalizeProviderPayload(payload, model, { mode: "chat" });

      expect(result.options).toEqual({ num_ctx: 262144 });
    });

    it("respects user-set contextWindow (manual override)", () => {
      // 用户在设置页手动改成 32768，应透传给 ollama
      const model = { ...ollamaModel, contextWindow: 32768 };
      const payload = {
        model: "gemma4:31b-nvfp4",
        messages: [{ role: "user", content: "hi" }],
      };

      const result = normalizeProviderPayload(payload, model, { mode: "chat" });

      expect(result.options.num_ctx).toBe(32768);
    });

    it("does not inject num_ctx when contextWindow is absent", () => {
      const payload = {
        model: "gemma4:12b-nvfp4",
        messages: [{ role: "user", content: "hi" }],
      };

      const result = normalizeProviderPayload(payload, ollamaModel, { mode: "chat" });

      expect(result.options).toBeUndefined();
    });

    it("does not inject num_ctx when contextWindow is 0 or invalid", () => {
      const model = { ...ollamaModel, contextWindow: 0 };
      const payload = {
        model: "gemma4:12b-nvfp4",
        messages: [{ role: "user", content: "hi" }],
      };

      const result = normalizeProviderPayload(payload, model, { mode: "chat" });

      expect(result.options).toBeUndefined();
    });

    it.each([8192.5, Number.POSITIVE_INFINITY, 1_048_577])(
      "does not inject num_ctx for unsafe contextWindow %s",
      (contextWindow) => {
        const model = { ...ollamaModel, contextWindow };
        const payload = {
          model: "gemma4:12b-nvfp4",
          messages: [{ role: "user", content: "hi" }],
          options: { temperature: 0.2 },
        };

        const result = normalizeProviderPayload(payload, model, { mode: "chat" });

        expect(result.options).toEqual({ temperature: 0.2 });
      },
    );

    it("merges num_ctx with existing options without clobbering them", () => {
      const model = { ...ollamaModel, contextWindow: 131072 };
      const payload = {
        model: "gemma4:12b-nvfp4",
        messages: [{ role: "user", content: "hi" }],
        options: { temperature: 0.7, top_p: 0.95 },
      };

      const result = normalizeProviderPayload(payload, model, { mode: "chat" });

      expect(result.options).toEqual({ temperature: 0.7, top_p: 0.95, num_ctx: 131072 });
    });

    it("can inject both response_format and num_ctx together", () => {
      const model = { ...ollamaModel, contextWindow: 8192 };
      const payload = {
        model: "gemma4:12b-nvfp4",
        messages: [{ role: "user", content: "describe" }],
      };

      const result = normalizeProviderPayload(payload, model, {
        mode: "chat",
        responseSchema: { type: "object" },
      });

      expect(result.response_format).toBeDefined();
      expect(result.options.num_ctx).toBe(8192);
    });
  });
});
