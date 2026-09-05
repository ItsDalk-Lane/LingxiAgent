import { describe, expect, it } from "vitest";
import {
  MediaExecutionTargetError,
  resolveMediaExecutionTarget,
} from "../core/media/media-execution-target-resolver.ts";

type ProviderFixture = {
  authType: "api-key" | "oauth" | "none" | "optional";
  apiKey?: string;
  headers?: Record<string, string>;
};

function providerRegistry({
  providers,
  activeProviderId = null,
  activeLaneId = null,
  lanes = [],
  unavailableReason = "no_credentials",
}: {
  providers: Record<string, ProviderFixture>;
  activeProviderId?: string | null;
  activeLaneId?: string | null;
  lanes?: Array<{ id: string; providerId: string; authType?: ProviderFixture["authType"] }>;
  unavailableReason?: string;
}) {
  return {
    get: (providerId: string) => providers[providerId] || null,
    getAuthType: (providerId: string) => providers[providerId]?.authType || "api-key",
    getCredentials: (providerId: string) => {
      const provider = providers[providerId];
      if (!provider) return null;
      return {
        apiKey: provider.apiKey || "",
        headers: provider.headers || {},
      };
    },
    getMediaProviderCredentialStatus: () => ({
      hasCredentials: !!activeProviderId,
      activeProviderId,
      activeLaneId,
      lanes,
      unavailableReason,
    }),
  };
}

function baseInput(registry: ReturnType<typeof providerRegistry>) {
  return {
    modelId: "doubao-seedream",
    modality: "image" as const,
    runtimeProviderId: "volcengine-runtime",
    adapterId: "volcengine-images",
    providerRegistry: registry,
  };
}

describe("媒体执行目标凭证解析", () => {
  it("显式且可用的模型凭证通道优先于注册表 active provider", () => {
    const registry = providerRegistry({
      providers: {
        "volcengine-runtime": { authType: "none" },
        volcengine: { authType: "api-key", apiKey: "explicit-secret" },
        "volcengine-coding": { authType: "api-key", apiKey: "active-secret" },
      },
      activeProviderId: "volcengine-coding",
      activeLaneId: "coding-plan",
      lanes: [
        { id: "api-plan", providerId: "volcengine" },
        { id: "coding-plan", providerId: "volcengine-coding" },
      ],
    });

    expect(resolveMediaExecutionTarget({
      ...baseInput(registry),
      credentialLane: { id: "api-plan", providerId: "volcengine" },
    })).toEqual({
      modelId: "doubao-seedream",
      modality: "image",
      runtimeProviderId: "volcengine-runtime",
      credentialProviderId: "volcengine",
      credentialLaneId: "api-plan",
      credentialSource: "provider-registry",
      adapterId: "volcengine-images",
      resolutionReason: "explicit_credential_lane",
    });
  });

  it("显式通道不可用时使用注册表 active provider", () => {
    const registry = providerRegistry({
      providers: {
        "volcengine-runtime": { authType: "api-key" },
        volcengine: { authType: "api-key" },
        "volcengine-coding": { authType: "api-key", apiKey: "active-secret" },
      },
      activeProviderId: "volcengine-coding",
      activeLaneId: "coding-plan",
      lanes: [
        { id: "api-plan", providerId: "volcengine" },
        { id: "coding-plan", providerId: "volcengine-coding" },
      ],
    });

    expect(resolveMediaExecutionTarget({
      ...baseInput(registry),
      modality: "video",
      credentialLane: { id: "api-plan", providerId: "volcengine" },
    })).toMatchObject({
      modality: "video",
      credentialProviderId: "volcengine-coding",
      credentialLaneId: "coding-plan",
      resolutionReason: "active_provider_registry_lane",
    });
  });

  it("无显式通道时使用注册表 active provider", () => {
    const registry = providerRegistry({
      providers: {
        "volcengine-runtime": { authType: "api-key" },
        volcengine: { authType: "api-key", apiKey: "active-secret" },
      },
      activeProviderId: "volcengine",
      activeLaneId: "volcengine-api",
      lanes: [{ id: "volcengine-api", providerId: "volcengine" }],
    });

    expect(resolveMediaExecutionTarget(baseInput(registry))).toMatchObject({
      credentialProviderId: "volcengine",
      credentialLaneId: "volcengine-api",
      resolutionReason: "active_provider_registry_lane",
    });
  });

  it("runtime provider 只有无认证或自身有凭证时才能兜底", () => {
    const noAuthRegistry = providerRegistry({
      providers: { "volcengine-runtime": { authType: "none" } },
    });
    const credentialRegistry = providerRegistry({
      providers: {
        "volcengine-runtime": { authType: "api-key", headers: { Authorization: "secret" } },
      },
    });

    expect(resolveMediaExecutionTarget({
      ...baseInput(noAuthRegistry),
      modality: "speech-recognition",
    })).toMatchObject({
      credentialProviderId: "volcengine-runtime",
      credentialLaneId: null,
      credentialSource: "none",
      resolutionReason: "runtime_provider_auth_none",
    });
    expect(resolveMediaExecutionTarget(baseInput(credentialRegistry))).toMatchObject({
      credentialProviderId: "volcengine-runtime",
      credentialSource: "provider-registry",
      resolutionReason: "runtime_provider_credentials",
    });
  });

  it("已知供应商缺凭证与未知供应商使用不同稳定错误码", () => {
    const missing = providerRegistry({
      providers: { "volcengine-runtime": { authType: "api-key" } },
      unavailableReason: "no_credentials",
    });
    const unresolved = providerRegistry({ providers: {} });

    for (const [registry, code] of [
      [missing, "CREDENTIAL_MISSING"],
      [unresolved, "CREDENTIAL_PROVIDER_UNRESOLVED"],
    ] as const) {
      expect(() => resolveMediaExecutionTarget(baseInput(registry))).toThrowError(
        expect.objectContaining({
          name: "MediaExecutionTargetError",
          code,
          details: expect.objectContaining({ resolutionReason: expect.any(String) }),
        }) as MediaExecutionTargetError,
      );
    }
  });
});
