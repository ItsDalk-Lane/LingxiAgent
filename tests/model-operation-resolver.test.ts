import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { normalizeSharedModelsPatch } from "../core/config-coordinator.ts";
import {
  ModelOperationConfigurationError,
  ModelOperationResolver,
} from "../core/model-operation-resolver.ts";
import { ProviderRegistry } from "../core/provider-registry.ts";

const tempRoots: string[] = [];

function tempRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-model-operation-"));
  tempRoots.push(root);
  return root;
}

afterEach(() => {
  for (const root of tempRoots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

describe("Provider operation model metadata", () => {
  it("不再预置任何内置操作模型卡，嵌入/重排目录纯由用户打标签驱动", () => {
    const registry = new ProviderRegistry(tempRoot());

    expect(registry.getOperationModelCatalog("embedding")).toEqual([]);
    expect(registry.getOperationModelCatalog("rerank")).toEqual([]);
  });

  it("用户打标签的重排条目进入操作目录并按供应商推断协议", () => {
    const registry = new ProviderRegistry(tempRoot());
    registry.saveProvider("siliconflow", {
      models: [{ id: "BAAI/bge-reranker-v2-m3", operations: ["rerank"] }],
    });

    expect(registry.getOperationModelCatalog("rerank")).toEqual([
      expect.objectContaining({
        id: "BAAI/bge-reranker-v2-m3",
        provider: "siliconflow",
        operations: ["rerank"],
        operationProtocol: "cohere-rerank",
      }),
    ]);
  });
});

describe("ModelOperationResolver", () => {
  function makeResolver(ref: any, model: any, freshCredential: any) {
    const refresh = vi.fn(async () => freshCredential);
    const resolver = new ModelOperationResolver({
      getOperationModelRef: () => ref,
      resolveOperationModel: () => model,
      resolveProviderCredentialsFresh: refresh,
      getProviderCredentials: () => freshCredential,
    });
    return { resolver, refresh };
  }

  it("treats an unconfigured operation as optional", async () => {
    const { resolver, refresh } = makeResolver(null, null, null);
    await expect(resolver.resolveFresh("embedding")).resolves.toBeNull();
    expect(refresh).not.toHaveBeenCalled();
  });

  it("refreshes the selected provider credential at the request boundary", async () => {
    const model = {
      id: "embed-model",
      provider: "provider-a",
      operations: ["embedding"],
      operationProtocol: "openai-embeddings",
      baseUrl: "https://provider-a.example/v1",
    };
    const { resolver, refresh } = makeResolver(
      { id: model.id, provider: model.provider },
      model,
      {
        apiKey: "fresh-key",
        baseUrl: "https://provider-a.example/v1",
        credentialSource: "provider-catalog",
      },
    );

    await expect(resolver.resolveFresh("embedding")).resolves.toMatchObject({
      operation: "embedding",
      api: "openai-embeddings",
      apiKey: "fresh-key",
      provider: "provider-a",
      model: { id: "embed-model", provider: "provider-a" },
    });
    expect(refresh).toHaveBeenCalledOnce();
    expect(refresh).toHaveBeenCalledWith("provider-a");
  });

  it("fails closed when a configured model disappears", async () => {
    const { resolver } = makeResolver(
      { id: "gone", provider: "provider-a" },
      null,
      { apiKey: "key", baseUrl: "https://provider-a.example/v1" },
    );
    await expect(resolver.resolveFresh("rerank")).rejects.toMatchObject({
      name: "ModelOperationConfigurationError",
      code: "model_not_found",
      operation: "rerank",
    } satisfies Partial<ModelOperationConfigurationError>);
  });
});

describe("ModelOperationResolver registry integration", () => {
  // 走真实 ProviderRegistry 集成路径:模型条目来自用户 catalog 配置(saveProvider 注入),
  // 用户只打操作标签,协议按供应商推断;仅凭证手工注入
  function makeRegistryResolver(registry: ProviderRegistry, ref: any, credential: any) {
    return new ModelOperationResolver({
      getOperationModelRef: () => ref,
      resolveOperationModel: (operation, modelRef) => registry.getOperationModel(operation, modelRef),
      resolveProviderCredentialsFresh: async () => credential,
      getProviderCredentials: () => credential,
    });
  }

  const providerCatalogCredential = {
    apiKey: "test-key",
    credentialSource: "provider-catalog",
  };

  // ollama authType "none":无 apiKey,凭本地 baseUrl 兜底放行
  const localNoneCredential = {
    apiKey: "",
    baseUrl: "",
    credentialSource: "none",
  };

  it("ollama 自添加嵌入条目按供应商推断 ollama-embed 协议,baseUrl 继承默认 /v1(客户端剥前缀)", async () => {
    const registry = new ProviderRegistry(tempRoot());
    registry.saveProvider("ollama", {
      models: [{ id: "qwen3-embedding:8b", operations: ["embedding"], dimensions: 4096 }],
    });
    const resolver = makeRegistryResolver(
      registry,
      { id: "qwen3-embedding:8b", provider: "ollama" },
      localNoneCredential,
    );

    await expect(resolver.resolveFresh("embedding")).resolves.toMatchObject({
      operation: "embedding",
      api: "ollama-embed",
      baseUrl: "http://localhost:11434/v1",
      model: { id: "qwen3-embedding:8b", dimensions: 4096 },
    });
  });

  it("gemini 自添加嵌入条目按供应商推断 gemini-embed 协议", async () => {
    const registry = new ProviderRegistry(tempRoot());
    registry.saveProvider("gemini", {
      models: [{ id: "gemini-embedding-001", operations: ["embedding"] }],
    });
    const resolver = makeRegistryResolver(
      registry,
      { id: "gemini-embedding-001", provider: "gemini" },
      providerCatalogCredential,
    );

    await expect(resolver.resolveFresh("embedding")).resolves.toMatchObject({
      operation: "embedding",
      api: "gemini-embed",
      baseUrl: "https://generativelanguage.googleapis.com/v1beta",
    });
  });

  it("dashscope 自添加重排条目推断 dashscope-rerank 双端点方言", async () => {
    const registry = new ProviderRegistry(tempRoot());
    registry.saveProvider("dashscope", {
      models: [
        { id: "gte-rerank-v2", operations: ["rerank"] },
        { id: "qwen3-rerank", operations: ["rerank"] },
      ],
    });

    // 双端点方言内部按模型 id 分流（原生嵌套 vs 兼容扁平），目录层统一标记同一协议
    for (const modelId of ["gte-rerank-v2", "qwen3-rerank"]) {
      const resolver = makeRegistryResolver(
        registry,
        { id: modelId, provider: "dashscope" },
        providerCatalogCredential,
      );
      await expect(resolver.resolveFresh("rerank")).resolves.toMatchObject({
        operation: "rerank",
        api: "dashscope-rerank",
        baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      });
    }
  });

  it("minimax 自添加嵌入条目按供应商推断 minimax-embeddings 协议", async () => {
    const registry = new ProviderRegistry(tempRoot());
    registry.saveProvider("minimax", {
      models: [{ id: "embo-01", operations: ["embedding"] }],
    });
    const resolver = makeRegistryResolver(
      registry,
      { id: "embo-01", provider: "minimax" },
      providerCatalogCredential,
    );

    await expect(resolver.resolveFresh("embedding")).resolves.toMatchObject({
      operation: "embedding",
      api: "minimax-embeddings",
      baseUrl: "https://api.minimaxi.com/anthropic",
    });
  });

  it("OpenAI 兼容供应商的自添加嵌入条目回退通用 openai-embeddings 协议", async () => {
    const registry = new ProviderRegistry(tempRoot());
    registry.saveProvider("deepseek", {
      models: [{ id: "custom-embed", operations: ["embedding"], dimensions: 2048 }],
    });
    const resolver = makeRegistryResolver(
      registry,
      { id: "custom-embed", provider: "deepseek" },
      providerCatalogCredential,
    );

    const resolved = await resolver.resolveFresh("embedding");
    expect(resolved).toMatchObject({
      operation: "embedding",
      api: "openai-embeddings",
      model: { id: "custom-embed", dimensions: 2048 },
    });
  });

  it("用户显式声明的 operationProtocol 优先于供应商推断", async () => {
    const registry = new ProviderRegistry(tempRoot());
    registry.saveProvider("deepseek", {
      models: [{ id: "custom-rerank", operations: ["rerank"], operationProtocol: "siliconflow-rerank" }],
    });
    const resolver = makeRegistryResolver(
      registry,
      { id: "custom-rerank", provider: "deepseek" },
      providerCatalogCredential,
    );

    await expect(resolver.resolveFresh("rerank")).resolves.toMatchObject({
      operation: "rerank",
      api: "siliconflow-rerank",
    });
  });
});

describe("ModelOperationResolver credential routing boundaries (P04)", () => {
  // P04-A01/A02/A04/A05：模型身份=provider+id 联合键；缺凭证 fail-closed 不偷回退；
  // 配置轮换后按当前修订取新凭证；合法无 apiKey 模式（本地端点/裸 header 凭证）放行。
  function makeResolver(ref: any, model: any, freshCredential: any) {
    const refresh = vi.fn(async () => freshCredential);
    const resolver = new ModelOperationResolver({
      getOperationModelRef: () => ref,
      resolveOperationModel: () => model,
      resolveProviderCredentialsFresh: refresh,
      getProviderCredentials: () => freshCredential,
    });
    return { resolver, refresh };
  }

  const sharedModelId = "shared-embed";

  function embeddingModel(provider: string, baseUrl: string) {
    return {
      id: sharedModelId,
      provider,
      operations: ["embedding"],
      operationProtocol: "openai-embeddings",
      baseUrl,
    };
  }

  it("P04-A01：同名 modelId 的两个 provider 不互串——解析与凭证刷新都锁定配置的 provider", async () => {
    // 走真实 ProviderRegistry（生产 getOperationModel 联合键匹配），凭证按 provider 分道
    const registry = new ProviderRegistry(tempRoot());
    registry.saveProvider("provider-a", {
      base_url: "https://provider-a.example/v1",
      models: [{ id: sharedModelId, operations: ["embedding"] }],
    });
    registry.saveProvider("provider-b", {
      base_url: "https://provider-b.example/v1",
      models: [{ id: sharedModelId, operations: ["embedding"] }],
    });
    const refresh = vi.fn(async (provider: string) => ({
      apiKey: `key-${provider}`,
      baseUrl: `https://${provider}.example/v1`,
      credentialSource: "provider-catalog",
    }));
    const makeResolverFor = (provider: string) => new ModelOperationResolver({
      getOperationModelRef: () => ({ id: sharedModelId, provider }),
      resolveOperationModel: (operation, ref) => registry.getOperationModel(operation, ref),
      resolveProviderCredentialsFresh: refresh,
      getProviderCredentials: () => null,
    });

    const resolved = await makeResolverFor("provider-a").resolveFresh("embedding");
    expect(resolved).toMatchObject({
      provider: "provider-a",
      apiKey: "key-provider-a",
      baseUrl: "https://provider-a.example/v1",
    });
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(refresh).toHaveBeenCalledWith("provider-a");

    // 反向引用同样不串：provider-b 的模型与凭证
    const reverseResolved = await makeResolverFor("provider-b").resolveFresh("embedding");
    expect(reverseResolved).toMatchObject({
      provider: "provider-b",
      apiKey: "key-provider-b",
      baseUrl: "https://provider-b.example/v1",
    });
    expect(refresh).toHaveBeenLastCalledWith("provider-b");
    expect(resolved.apiKey).not.toBe(reverseResolved.apiKey);
  });

  it("P04-A02：指定 provider 缺凭证 fail-closed——不退到另一 provider，也不发起请求", async () => {
    const refresh = vi.fn(async (provider: string) => ({
      apiKey: provider === "provider-a" ? "" : "key-provider-b",
      baseUrl: `https://${provider}.example/v1`,
      credentialSource: "provider-catalog",
    }));
    const resolver = new ModelOperationResolver({
      getOperationModelRef: () => ({ id: sharedModelId, provider: "provider-a" }),
      resolveOperationModel: () => embeddingModel("provider-a", "https://provider-a.example/v1"),
      resolveProviderCredentialsFresh: refresh,
      getProviderCredentials: () => null,
    });

    await expect(resolver.resolveFresh("embedding")).rejects.toMatchObject({
      name: "ModelOperationConfigurationError",
      code: "provider_missing_creds",
      operation: "embedding",
    } satisfies Partial<ModelOperationConfigurationError>);
    // 凭证刷新只发生在被选中的 provider-a 上；provider-b 的凭证源从未被触碰
    // （resolver 层面即无第二跳，EmbeddingClient 的 fetch 更不可能发出）。
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(refresh).toHaveBeenCalledWith("provider-a");
  });

  it("P04-A04：配置轮换后 resolveFresh 按当前修订取新凭证，不沿用旧快照", async () => {
    let currentKey = "key-old";
    const refresh = vi.fn(async () => ({
      apiKey: currentKey,
      baseUrl: "https://provider-a.example/v1",
      credentialSource: "provider-catalog",
    }));
    const resolver = new ModelOperationResolver({
      getOperationModelRef: () => ({ id: sharedModelId, provider: "provider-a" }),
      resolveOperationModel: () => embeddingModel("provider-a", "https://provider-a.example/v1"),
      resolveProviderCredentialsFresh: refresh,
      getProviderCredentials: () => null,
    });

    const first = await resolver.resolveFresh("embedding");
    expect(first.apiKey).toBe("key-old");

    // 用户在设置里轮换了 key（provider catalog 更新）
    currentKey = "key-rotated";
    const second = await resolver.resolveFresh("embedding");
    expect(second.apiKey).toBe("key-rotated");
    // 错误信息只含 provider 名，不含任何密钥值
    expect(first.apiKey).not.toEqual(second.apiKey);
  });

  it("P04-A05：本地端点无 apiKey（isLocalBaseUrl 默认规则）合法放行", async () => {
    const { resolver } = makeResolver(
      { id: sharedModelId, provider: "local-runtime" },
      embeddingModel("local-runtime", "http://127.0.0.1:11434/v1"),
      { apiKey: "", baseUrl: "http://127.0.0.1:11434/v1", credentialSource: "none" },
    );
    await expect(resolver.resolveFresh("embedding")).resolves.toMatchObject({
      provider: "local-runtime",
      api: "openai-embeddings",
      baseUrl: "http://127.0.0.1:11434/v1",
    });
  });

  it("P04-A05：仅 header 凭证（无 apiKey）的远程 provider 合法放行，header 进入执行配置", async () => {
    const { resolver } = makeResolver(
      { id: sharedModelId, provider: "header-auth-provider" },
      embeddingModel("header-auth-provider", "https://header-auth.example/v1"),
      {
        apiKey: "",
        baseUrl: "https://header-auth.example/v1",
        headers: { "x-custom-auth": "header-secret" },
        credentialSource: "provider-catalog",
      },
    );
    await expect(resolver.resolveFresh("embedding")).resolves.toMatchObject({
      provider: "header-auth-provider",
      headers: { "x-custom-auth": "header-secret" },
    });
  });
});

describe("operation preference normalization", () => {
  it("v8 起知识库嵌入/重排全局字段被显式拒绝（迁移至笔记本级配置）", () => {
    // 旧客户端 PUT embedding/rerank → unknown field 400（显式拒绝，禁静默降级）。
    expect(() => normalizeSharedModelsPatch({
      embedding: { id: "embed", provider: "provider-a" },
    })).toThrow(/unknown shared model field "embedding"/);
    expect(() => normalizeSharedModelsPatch({ rerank: null })).toThrow(
      /unknown shared model field "rerank"/,
    );
    expect(() => normalizeSharedModelsPatch({ embedding: "embed" })).toThrow(
      /unknown shared model field "embedding"/,
    );
  });
});
