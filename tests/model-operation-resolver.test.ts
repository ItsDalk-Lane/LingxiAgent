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
  it("keeps embedding and rerank models outside the chat catalog", () => {
    const registry = new ProviderRegistry(tempRoot());
    const embedding = registry.getOperationModelCatalog("embedding");
    const rerank = registry.getOperationModelCatalog("rerank");

    expect(embedding).toEqual(expect.arrayContaining([
      expect.objectContaining({
        id: "text-embedding-3-small",
        provider: "openai",
        operations: ["embedding"],
        operationProtocol: "openai-embeddings",
      }),
    ]));
    expect(rerank).toEqual(expect.arrayContaining([
      expect.objectContaining({
        id: "BAAI/bge-reranker-v2-m3",
        provider: "siliconflow",
        operations: ["rerank"],
        operationProtocol: "siliconflow-rerank",
      }),
    ]));
    expect(registry.getChatDiscoverableModelEntries("openai")).not.toContainEqual(
      expect.objectContaining({ id: "text-embedding-3-small" }),
    );
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
