import { afterEach, describe, expect, it, vi } from "vitest";
import {
  appendProviderApiPath,
  normalizeProviderBaseUrlForApi,
  probeProvider,
} from "../lib/llm/provider-client.ts";
import { createTemporaryProviderCredentialBoundary } from "../core/temporary-provider-credential-boundary.ts";

function probeCredentialBoundary(providerId = "opencode") {
  return createTemporaryProviderCredentialBoundary({
    providerId,
    source: "request-draft",
    operation: "connectivity-probe",
    apiKey: "test-key",
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("provider endpoint handling", () => {
  it("appends an API path at most once", () => {
    expect(appendProviderApiPath("https://opencode.ai/zen", "/v1/messages"))
      .toBe("https://opencode.ai/zen/v1/messages");
    expect(appendProviderApiPath("https://opencode.ai/zen/v1", "/v1/messages"))
      .toBe("https://opencode.ai/zen/v1/messages");
    expect(appendProviderApiPath("https://opencode.ai/zen/v1/messages", "/v1/messages"))
      .toBe("https://opencode.ai/zen/v1/messages");
  });

  it("derives an Anthropic SDK base without rewriting unrelated path segments", () => {
    expect(normalizeProviderBaseUrlForApi({
      baseUrl: "https://gateway.example/tenant/anthropic/v1/messages",
      api: "anthropic-messages",
    })).toBe("https://gateway.example/tenant/anthropic");
    expect(normalizeProviderBaseUrlForApi({
      baseUrl: "https://gateway.example/tenant/anthropic/v1",
      api: "anthropic-messages",
    })).toBe("https://gateway.example/tenant/anthropic");
  });

  it("derives OpenAI SDK bases from complete user-entered endpoints", () => {
    expect(normalizeProviderBaseUrlForApi({
      baseUrl: "https://gateway.example/tenant/v1/chat/completions",
      api: "openai-completions",
    })).toBe("https://gateway.example/tenant/v1");
    expect(normalizeProviderBaseUrlForApi({
      baseUrl: "https://gateway.example/tenant/v1/responses",
      api: "openai-responses",
    })).toBe("https://gateway.example/tenant/v1");
    expect(normalizeProviderBaseUrlForApi({
      provider: "kimi-coding",
      baseUrl: "https://api.kimi.com/coding/v1/chat/completions",
      api: "openai-completions",
    })).toBe("https://api.kimi.com/coding/v1");
  });

  it("normalizes OpenCode Zen per protocol from either a root or full Messages endpoint", () => {
    for (const configured of [
      "https://opencode.ai/zen",
      "https://opencode.ai/zen/v1/messages",
    ]) {
      expect(normalizeProviderBaseUrlForApi({
        provider: "opencode",
        baseUrl: configured,
        api: "anthropic-messages",
      })).toBe("https://opencode.ai/zen");
      expect(normalizeProviderBaseUrlForApi({
        provider: "opencode",
        baseUrl: configured,
        api: "openai-completions",
      })).toBe("https://opencode.ai/zen/v1");
      expect(normalizeProviderBaseUrlForApi({
        provider: "opencode",
        baseUrl: configured,
        api: "openai-responses",
      })).toBe("https://opencode.ai/zen/v1");
    }
  });
});

describe("provider connectivity probe", () => {
  it("uses the configured model and accepts only a successful HTTP response", async () => {
    const fetchMock = vi.fn(async (_url, init) => {
      expect(JSON.parse(String(init?.body))).toMatchObject({ model: "claude-sonnet-4-6" });
      return new Response("{}", { status: 200, headers: { "Content-Type": "application/json" } });
    });
    vi.stubGlobal("fetch", fetchMock);
    const usageLedger = {
      start: vi.fn(() => ({ requestId: "provider-probe-1" })),
      finish: vi.fn(),
      recordError: vi.fn(),
    };

    await expect(probeProvider({
      providerId: "opencode",
      baseUrl: "https://opencode.ai/zen/v1/messages",
      api: "anthropic-messages",
      credentialBoundary: probeCredentialBoundary(),
      modelId: "claude-sonnet-4-6",
      usageLedger,
      usageContext: {
        source: { subsystem: "provider-management", operation: "connectivity-probe", surface: "settings", trigger: "user" },
        attribution: { kind: "provider", providerId: "opencode" },
      },
    })).resolves.toEqual({ ok: true, status: 200 });
    expect(fetchMock).toHaveBeenCalledWith(
      "https://opencode.ai/zen/v1/messages",
      expect.any(Object),
    );
    expect(usageLedger.start).toHaveBeenCalledWith(expect.objectContaining({
      model: { provider: "opencode", modelId: "claude-sonnet-4-6", api: "anthropic-messages" },
    }));
    expect(usageLedger.finish).toHaveBeenCalledWith("provider-probe-1", expect.any(Object));
  });

  it("rejects 404 HTML instead of treating it as connectivity success", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("<html>OpenCode</html>", {
      status: 404,
      statusText: "Not Found",
      headers: { "Content-Type": "text/html" },
    })));

    await expect(probeProvider({
      providerId: "opencode",
      baseUrl: "https://opencode.ai/zen",
      api: "anthropic-messages",
      credentialBoundary: probeCredentialBoundary(),
      modelId: "claude-sonnet-4-6",
    })).resolves.toEqual({
      ok: false,
      status: 404,
      error: "HTTP 404 Not Found",
    });
  });
});
