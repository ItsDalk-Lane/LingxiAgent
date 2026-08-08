/**
 * AuxiliaryModelResolver fresh credential routing.
 *
 * 验证 Slot 解析在请求边界刷新 provider 凭证，且不同 provider 的 Slot 互不串扰。
 */
import { describe, expect, it, vi } from "vitest";

import { AuxiliaryModelResolver } from "../core/auxiliary-model-resolver.ts";

function makeResolver(models, freshResolver, options: any = {}) {
  const modelList = Object.values(models) as any[];
  const resolveModel = (ref: any) => {
    if (!ref || typeof ref !== "object") return null;
    return modelList.find((m) => m.id === ref.id && m.provider === ref.provider) || null;
  };
  const getChatModel = () => options.chatModel ?? null;
  const getSlotModelRef = (slot: string) => options.slotRefs?.[slot] || null;
  const getProviderCredentials = () => null;
  return new AuxiliaryModelResolver({
    resolveModel,
    getChatModel,
    getSlotModelRef,
    resolveProviderCredentialsFresh: freshResolver,
    getProviderCredentials,
  });
}

describe("AuxiliaryModelResolver fresh credential routing", () => {
  it("refreshes the provider credential and keeps the model API authoritative", async () => {
    const models = {
      summarize: {
        id: "small",
        provider: "oauth-runtime",
        api: "openai-responses",
        headers: {
          Authorization: "Bearer model-stale",
          "x-grok-model-override": "small",
        },
      },
    };
    const fresh = vi.fn(async () => ({
      apiKey: "fresh-token",
      baseUrl: "https://fresh.example/v1",
      api: "provider-default",
      headers: {
        Authorization: "Bearer provider-stale",
        "x-grok-client-version": "0.2.95",
      },
      accountId: "acct_fresh",
      credentialSource: "auth-storage",
    }));
    const resolver = makeResolver(models, fresh, {
      slotRefs: { summarize: { id: "small", provider: "oauth-runtime" } },
    });

    const resolved = await resolver.resolveAuxiliaryModelFresh("summarize");

    expect(fresh).toHaveBeenCalledOnce();
    expect(fresh).toHaveBeenCalledWith("oauth-runtime");
    expect(resolved).toMatchObject({
      api: "openai-responses",
      apiKey: "fresh-token",
      baseUrl: "https://fresh.example/v1",
    });
    // model API wins over provider default
    expect(resolved.headers).toEqual({
      "x-grok-client-version": "0.2.95",
      "x-grok-model-override": "small",
    });
    expect(resolved.model).toMatchObject({ accountId: "acct_fresh", headers: resolved.headers });
  });

  it("refreshes different slot providers independently", async () => {
    const models = {
      summarize: { id: "small", provider: "provider-a", api: "api-small" },
      memory: { id: "large", provider: "provider-b", api: "api-large" },
    };
    const fresh = vi.fn(async (provider: string) => ({
      apiKey: `key-${provider}`,
      baseUrl: `https://${provider}.example/v1`,
      api: `default-${provider}`,
      headers: {},
      credentialSource: "provider-catalog",
    }));
    const resolver = makeResolver(models, fresh, {
      slotRefs: {
        summarize: { id: "small", provider: "provider-a" },
        memory: { id: "large", provider: "provider-b" },
      },
    });

    const summarize = await resolver.resolveAuxiliaryModelFresh("summarize");
    const memory = await resolver.resolveAuxiliaryModelFresh("memory");

    expect(fresh.mock.calls.map(([provider]) => provider)).toEqual(["provider-a", "provider-b"]);
    expect(summarize).toMatchObject({
      api: "api-small",
      apiKey: "key-provider-a",
    });
    expect(memory).toMatchObject({
      api: "api-large",
      apiKey: "key-provider-b",
    });
  });

  it("fails closed when the fresh credential resolver is missing", async () => {
    const models = {
      summarize: { id: "small", provider: "provider-a", api: "api-small" },
    };
    const resolver = makeResolver(models, null as any, {
      slotRefs: { summarize: { id: "small", provider: "provider-a" } },
    });

    await expect(resolver.resolveAuxiliaryModelFresh("summarize")).rejects.toThrow();
  });

  it("returns null for an unconfigured slot with no chat fallback", async () => {
    const models = {
      approval: { id: "approval-model", provider: "provider-a", api: "api-approval" },
    };
    const fresh = vi.fn(async () => ({
      apiKey: "key",
      baseUrl: "https://example/v1",
      api: "openai",
      headers: {},
      credentialSource: "provider-catalog",
    }));
    // approval slot unconfigured, no chat fallback (fallback: none)
    const resolver = makeResolver(models, fresh, { slotRefs: {} });

    const resolved = await resolver.resolveAuxiliaryModelFresh("approval");

    expect(resolved).toBeNull();
    expect(fresh).not.toHaveBeenCalled();
  });
});
