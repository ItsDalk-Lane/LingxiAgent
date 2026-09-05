import fs from "node:fs";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import { MediaAdapterRegistry } from "../core/media-adapter-registry.ts";
import { ProviderRegistry } from "../core/provider-registry.ts";
import { runSubmitInBackground } from "../core/media/image-task-runner.ts";
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

  it("供应商注册表暴露唯一媒体执行目标门面", () => {
    expect(ProviderRegistry.prototype.resolveMediaExecutionTarget).toBeTypeOf("function");
  });

  it("适配器上下文绑定同一个规范媒体执行目标", () => {
    const registry = new MediaAdapterRegistry();
    registry.register({ id: "volcengine-images", types: ["image"] });
    const executionTarget = Object.freeze({
      modelId: "doubao-seedream",
      modality: "image" as const,
      runtimeProviderId: "volcengine-runtime",
      credentialProviderId: "volcengine",
      credentialLaneId: "api-plan",
      credentialSource: "provider-registry" as const,
      adapterId: "volcengine-images",
      resolutionReason: "explicit_credential_lane" as const,
    });

    expect(registry.createSubmitContextForExecutionTarget(executionTarget, { requestId: "one" }))
      .toMatchObject({ requestId: "one", mediaExecutionTarget: executionTarget });
  });

  it("image、video、STT 与后台 image task 都只调用统一解析器", () => {
    const root = path.resolve(import.meta.dirname, "..");
    for (const relativePath of [
      "hub/index.ts",
      "core/media/universal-media-manager.ts",
      "core/speech-recognition-service.ts",
      "core/media/image-task-runner.ts",
    ]) {
      const source = fs.readFileSync(path.join(root, relativePath), "utf8");
      expect(source, relativePath).toContain("resolveMediaExecutionTarget");
    }
  });

  it("后台图片任务执行前重新解析并只把新目标交给适配器", async () => {
    const submit = vi.fn(async () => ({ taskId: "provider-task" }));
    const adapter = { id: "volcengine-images", submit };
    const refreshedTarget = Object.freeze({
      modelId: "doubao-seedream",
      modality: "image" as const,
      runtimeProviderId: "volcengine-runtime",
      credentialProviderId: "volcengine-coding",
      credentialLaneId: "coding-plan",
      credentialSource: "provider-registry" as const,
      adapterId: "volcengine-images",
      resolutionReason: "active_provider_registry_lane" as const,
    });
    const refresh = vi.fn(() => refreshedTarget);
    const store = { get: vi.fn(() => ({})), update: vi.fn() };

    await runSubmitInBackground({
      taskId: "image-task",
      adapter,
      params: {
        prompt: "draw",
        providerId: "volcengine-runtime",
        modelId: "doubao-seedream",
        credentialProviderId: "volcengine",
      },
      submitCtx: {
        resolveMediaExecutionTarget: refresh,
        mediaAdapterRegistry: {
          createSubmitContextForExecutionTarget: (target, base) => ({
            ...base,
            mediaExecutionTarget: target,
          }),
        },
      },
      store,
      poller: { checkNow: vi.fn() },
      ctx: {
        bus: { request: vi.fn(async () => ({})) },
        log: { error: vi.fn() },
      },
    });

    expect(refresh).toHaveBeenCalledWith(expect.objectContaining({
      modelId: "doubao-seedream",
      modality: "image",
      runtimeProviderId: "volcengine-runtime",
      credentialLane: null,
      adapterId: "volcengine-images",
    }));
    expect(submit).toHaveBeenCalledWith(expect.objectContaining({
      credentialProviderId: "volcengine-coding",
      credentialLaneId: "coding-plan",
    }), expect.objectContaining({ mediaExecutionTarget: refreshedTarget }));
  });

  it("多通道适配器不会在所选凭证刷新失败后改试其它通道", async () => {
    const { volcengineImageAdapter } = await import("../core/media-adapters/volcengine.ts");
    const request = vi.fn(async (_type, payload) => (
      payload.providerId === "volcengine-coding"
        ? { error: "CREDENTIAL_REFRESH_TIMEOUT" }
        : { apiKey: "must-not-be-used" }
    ));

    await expect(volcengineImageAdapter.submit({
      prompt: "draw",
      modelId: "doubao-seedream-4-0-250828",
      credentialProviderId: "volcengine-coding",
    }, {
      bus: { request },
      config: { get: vi.fn(() => ({})) },
    })).rejects.toThrow();

    expect(request).toHaveBeenCalledTimes(1);
    expect(request).toHaveBeenCalledWith("provider:credentials", {
      providerId: "volcengine-coding",
    });
  });
});
