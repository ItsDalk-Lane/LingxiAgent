import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { KnowledgeQueryService } from "../lib/knowledge/knowledge-query-service.ts";
import { KnowledgeSearchService } from "../lib/knowledge/knowledge-search-service.ts";
import type { KnowledgeModelRef } from "../lib/knowledge/types.ts";
import { KNOWLEDGE_RERANK_DISABLED_POLICY } from "../lib/knowledge/rerank-policy.ts";

const homes: string[] = [], managers: KnowledgeManager[] = [];
const services: KnowledgeSearchService[] = [];
afterEach(async () => {
  vi.restoreAllMocks();
  for (const service of services.splice(0)) service.close();
  for (const manager of managers.splice(0)) await manager.close();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});
async function fixture(modelCount = 1) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-model-groups-")); homes.push(home);
  const manager = new KnowledgeManager({ lingxiHome: home }); managers.push(manager);
  const revisions = new Map([["embed-0", "1"], ["embed-1", "1"]]);
  const embed = vi.fn(async (input: { texts: string[]; modelRef: KnowledgeModelRef; signal?: AbortSignal }) => ({
    vectors: input.texts.map(() => [1, 0]), dimensions: 2,
    model: { provider: input.modelRef.provider, id: input.modelRef.id, api: "openai" },
  }));
  const query = new KnowledgeQueryService({ store: manager.store, indexStore: manager.indexStore,
    vectorIndex: manager.vectorIndex, embedTextsForModel: embed,
    getModelConfigurationRevision: ref => revisions.get(ref.id) ?? "1",
  });
  const notebookIds: string[] = [];
  for (let i = 0; i < 5; i += 1) {
    const notebook = manager.createNotebook({ studioId: "groups", name: `资料 ${i}` }); notebookIds.push(notebook.id);
    manager.updateNotebookSettings({ studioId: "groups", notebookId: notebook.id, chunkTargetChars: 200 });
    const imported = await manager.importPastedText({ studioId: "groups", notebookId: notebook.id,
      displayName: `资料${i}.txt`, text: `后台资料内容 ${i} 独立来源。` });
    const artifact = await manager.parseSource({ studioId: "groups", sourceId: imported.source.id });
    manager.enqueueSourceIngestion({ studioId: "groups", notebookId: notebook.id, sourceId: imported.source.id });
    await manager.ingestion.drainQueue();
    const modelRef = { provider: "fixture", id: `embed-${i % modelCount}` };
    manager.updateNotebookSettings({ studioId: "groups", notebookId: notebook.id, embeddingModelRef: modelRef });
    const indexed = query.indexArtifactForIngestion("groups", artifact.id, { targetChars: 200 });
    await query.embedArtifactForIngestion({ runId: "fixture-index", parseArtifactId: artifact.id,
      chunkProfileHash: indexed.chunkerConfigId, embedTexts: request => embed({ ...request, modelRef }) });
  }
  const scope = manager.createTurnScope({ studioId: "groups", sessionPath: "/tmp/group-session.jsonl", notebookIds });
  const compiledScope = await manager.compileTurnScope(scope);
  const search = new KnowledgeSearchService({ store: manager.store, indexStore: manager.indexStore, queryService: query }); services.push(search);
  embed.mockClear();
  return { manager, search, embed, revisions, query, request: { compiledScope, query: "后台", channel: "hybrid" as const,
    limit: 8, rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY } };
}

describe("搜索按嵌入模型分组", () => {
  it.each([1, 2])("五个独立来源使用 %i 种模型，每组一次嵌入、一次来源召回及一次章节补查", async modelCount => {
    const { manager, search, request, embed } = await fixture(modelCount);
    const vector = vi.spyOn(manager.vectorIndex, "search");
    const result = await search.search(request);
    expect(embed).toHaveBeenCalledTimes(modelCount);
    expect(vector.mock.calls.filter(([input]) => input.chunkIds === undefined)).toHaveLength(modelCount);
    expect(vector.mock.calls.filter(([input]) => input.chunkIds !== undefined)).toHaveLength(modelCount);
    expect(result.remoteModelCalls).toBe(modelCount);
    expect(new Set(result.hits.map(hit => hit.sourceId)).size).toBe(5);
    for (const exact of [false, true]) {
      expect(vector.mock.calls.filter(([input]) => (input.chunkIds !== undefined) === exact)
        .reduce((count, [input]) => count + (input.vectorIndexVariantIds as string[]).length, 0)).toBe(5);
    }
    const again = await search.search(request);
    expect(again.remoteModelCalls).toBe(0);
    expect(embed).toHaveBeenCalledTimes(modelCount);
    expect(vector.mock.calls.filter(([input]) => input.chunkIds === undefined)).toHaveLength(modelCount);
    expect(vector.mock.calls.filter(([input]) => input.chunkIds !== undefined)).toHaveLength(modelCount);
  });

  it("并发相同搜索仅一次底层执行，命中缓存仍校验范围关闭", async () => {
    const { manager, search, request, embed } = await fixture();
    const vector = vi.spyOn(manager.vectorIndex, "search");
    const results = await Promise.all(Array.from({ length: 5 }, () => search.search(request)));
    expect(embed).toHaveBeenCalledTimes(1); expect(vector.mock.calls.filter(([input]) => input.chunkIds === undefined)).toHaveLength(1);
    expect(vector.mock.calls.filter(([input]) => input.chunkIds !== undefined)).toHaveLength(1);
    expect(results.map(result => result.hits)).toEqual(Array.from({ length: 5 }, () => results[0].hits));
    manager.closeTurnScope({ scopeId: request.compiledScope.scopeId });
    await expect(search.search(request)).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
  });

  it("不同结果键仍共享查询向量；改一个模型配置只失效该模型", async () => {
    const { search, request, embed, revisions } = await fixture(2);
    await search.search(request);
    const reused = await search.search({ ...request, limit: 9 });
    expect(reused).toMatchObject({ queryEmbeddingCacheHit: true, retrievalResultCacheHit: false, embeddingGroups: 2, remoteModelCalls: 0 });
    expect(embed).toHaveBeenCalledTimes(2);
    revisions.set("embed-0", "2");
    search.refreshModelConfigurations();
    await search.search(request);
    expect(embed).toHaveBeenCalledTimes(3);
    expect(embed.mock.calls.at(-1)![0].modelRef.id).toBe("embed-0");
  });

  it("不同过滤条件并发共享嵌入，取消一方不会取消另一个请求", async () => {
    const { search, request, embed } = await fixture();
    const gate = Promise.withResolvers<void>();
    const original = embed.getMockImplementation()!;
    embed.mockImplementation(async input => { await gate.promise; input.signal?.throwIfAborted(); return original(input); });
    const controller = new AbortController();
    const first = search.search({ ...request, limit: 8, signal: controller.signal });
    const second = search.search({ ...request, limit: 9 });
    await vi.waitFor(() => expect(embed).toHaveBeenCalledTimes(1));
    controller.abort();
    await expect(first).rejects.toMatchObject({ name: "AbortError" });
    gate.resolve();
    expect((await second).hits).toHaveLength(5);
    expect(embed).toHaveBeenCalledTimes(1);
  });
});
