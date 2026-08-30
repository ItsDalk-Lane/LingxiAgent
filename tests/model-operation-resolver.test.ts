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

  it("dashscope 自添加重排条目按供应商推断 cohere-rerank 协议的兼容模式端点", async () => {
    const registry = new ProviderRegistry(tempRoot());
    registry.saveProvider("dashscope", {
      models: [{ id: "gte-rerank-v2", operations: ["rerank"] }],
    });
    const resolver = makeRegistryResolver(
      registry,
      { id: "gte-rerank-v2", provider: "dashscope" },
      providerCatalogCredential,
    );

    await expect(resolver.resolveFresh("rerank")).resolves.toMatchObject({
      operation: "rerank",
      api: "cohere-rerank",
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
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
