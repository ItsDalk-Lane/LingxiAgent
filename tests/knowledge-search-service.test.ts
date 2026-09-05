import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { KnowledgeQueryService } from "../lib/knowledge/knowledge-query-service.ts";
import { KnowledgeSearchService } from "../lib/knowledge/knowledge-search-service.ts";
import {
  KNOWLEDGE_RERANK_DISABLED_POLICY,
  KNOWLEDGE_RERANK_ENABLED_POLICY,
} from "../lib/knowledge/rerank-policy.ts";
import { createMetadataFixture, metadataStudio } from "./helpers/knowledge-metadata-fixture.ts";

import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish } from "./helpers/knowledge-research-agent-fixture.ts";
const researchFixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
const homes: string[] = [];
const managers: KnowledgeManager[] = [];
afterEach(async () => {
  vi.restoreAllMocks();
  for (const fixture of researchFixtures.splice(0)) await fixture.close();
  for (const manager of managers.splice(0)) await manager.close();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});
async function fixture() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-search-service-")); homes.push(home);
  const data = await createMetadataFixture(home); managers.push(data.manager);
  const compiledScope = await data.manager.compileTurnScope(data.scope);
  return { ...data, compiledScope, request: { compiledScope, query: "后台", channel: "fts" as const,
    rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY, limit: 8 } };
}

describe("统一知识搜索", () => {
  it("快速搜索由统一入口一次 FTS，只有命中块定点回读，候选携带冻结身份", async () => {
    const { manager, request, variant, imported } = await fixture();
    expect("retrieveForNotebooks" in manager.queryService).toBe(false);
    const legacy = vi.fn(async () => { throw new Error("禁止旧检索"); });
    Object.assign(manager.queryService, { retrieveForNotebooks: legacy });
    const hybrid = vi.spyOn(manager.queryService, "retrieveCompiledGroup").mockRejectedValue(new Error("禁止远程"));
    const fullRead = vi.spyOn(manager.store, "listArtifactBlocks").mockImplementation(() => { throw new Error("禁止全量读取"); });
    const fts = vi.spyOn(manager.indexStore, "searchReadyVariantIds");
    const result = await manager.searchService.search(request);
    expect(result.hits.length).toBeGreaterThan(0);
    expect(result.hits[0]).toMatchObject({ sourceId: imported.source.id, contentSnapshotId: imported.snapshot.id,
      chunkIndexVariantId: variant.id, channels: ["fts"] });
    expect(result.hits[0].candidateId).toMatch(/^kc_[a-f0-9]{32}$/);
    expect(result.hits[0]).not.toHaveProperty("evidenceId");
    expect(result.remoteModelCalls).toBe(0);
    expect(result.vectorBackend).toBe("none");
    expect(fts).toHaveBeenCalledTimes(1);
    for (const spy of [legacy, hybrid, fullRead]) expect(spy).not.toHaveBeenCalled();
  });

  it.each([
    { notebookIds: ["outside"] }, { sourceIds: ["outside"] }, { sectionKeys: ["outside"] },
  ])("范围外过滤 %j 在检索前拒绝", async filter => {
    const { manager, request } = await fixture();
    const fts = vi.spyOn(manager.indexStore, "searchReadyVariantIds");
    const hybrid = vi.spyOn(manager.queryService, "retrieveCompiledGroup");
    await expect(manager.searchService.search({ ...request, channel: "hybrid", ...filter })).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(fts).not.toHaveBeenCalled(); expect(hybrid).not.toHaveBeenCalled();
  });

  it("章节过滤只查命中章节；空源过滤返回空，不变成查全部", async () => {
    const { manager, request } = await fixture();
    const second = await manager.searchService.search({ ...request, sectionKeys: ["第二章"] });
    expect(second.hits).toHaveLength(1);
    expect(second.hits[0].snippet).toContain("后台");
    expect((await manager.searchService.search({ ...request, sourceIds: [] })).hits).toEqual([]);
  });

  it("关闭范围、跨会话、伪造冻结产物均拒绝", async () => {
    const { manager, request } = await fixture();
    await expect(manager.searchService.search({ ...request, compiledScope: { ...request.compiledScope, sessionPath: "/other" } }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    await expect(manager.searchService.search({ ...request, compiledScope: { ...request.compiledScope,
      sources: request.compiledScope.sources.map(source => ({ ...source, parseArtifactId: "wrong" })) } }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    manager.closeTurnScope({ scopeId: request.compiledScope.scopeId });
    await expect(manager.searchService.search(request)).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
  });

  it("取消和本地重排非法请求都不发起检索", async () => {
    const { manager, request } = await fixture();
    const fts = vi.spyOn(manager.indexStore, "searchReadyVariantIds");
    await expect(manager.searchService.search({ ...request, signal: AbortSignal.abort() })).rejects.toMatchObject({ name: "AbortError" });
    await expect(manager.searchService.search({ ...request, rerankPolicy: KNOWLEDGE_RERANK_ENABLED_POLICY }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_INVALID_ARGUMENT" });
    expect(fts).not.toHaveBeenCalled();
  });

  it("真实混合搜索保留两个召回通道，远程调用按实际执行计数", async () => {
    const { manager, notebook, variant, artifact, scope } = await fixture();
    const modelRef = { provider: "fixture", id: "embedding" };
    manager.updateNotebookSettings({ studioId: metadataStudio, notebookId: notebook.id, embeddingModelRef: modelRef });
    const embed = vi.fn(async (input: { texts: string[] }) => ({ vectors: input.texts.map(() => [1, 0]),
      dimensions: 2, model: { provider: "fixture", id: "embedding", api: "openai" } }));
    const queryService = new KnowledgeQueryService({ store: manager.store, indexStore: manager.indexStore,
      vectorIndex: manager.vectorIndex, embedTextsForModel: embed });
    await queryService.embedArtifactForIngestion({ runId: "fixture-index", parseArtifactId: artifact.id,
      chunkProfileHash: variant.chunkProfileHash, embedTexts: embed });
    embed.mockClear();
    const search = new KnowledgeSearchService({ store: manager.store, indexStore: manager.indexStore, queryService });
    const result = await search.search({ compiledScope: await manager.compileTurnScope(scope), query: "后台",
      channel: "hybrid", rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY, limit: 8 });
    expect(embed).toHaveBeenCalledTimes(1);
    expect(result.remoteModelCalls).toBe(1);
    expect(result.retrievalMode).toBe("hybrid");
    expect(result.vectorBackend).toBe("portable");
    expect(result.hits.some(hit => hit.channels.includes("fts") && hit.channels.includes("vector"))).toBe(true);
  });

  it("配置的模型未接通时明确说明降级", async () => {
    const { manager, notebook, scope } = await fixture();
    manager.updateNotebookSettings({ studioId: metadataStudio, notebookId: notebook.id,
      embeddingModelRef: { provider: "missing", id: "embed" }, rerankModelRef: { provider: "missing", id: "rerank" } });
    const result = await manager.searchService.search({ compiledScope: await manager.compileTurnScope(scope), query: "后台",
      channel: "hybrid", rerankPolicy: KNOWLEDGE_RERANK_ENABLED_POLICY, limit: 8 });
    expect(result.retrievalMode).toBe("fts");
    expect(result.remoteModelCalls).toBe(0);
    expect(result.degradedReasons.some(reason => reason.includes("KNOWLEDGE_VECTOR_NOT_READY"))).toBe(true);
    expect(result.degradedReasons.some(reason => reason.includes("rerank model is unavailable"))).toBe(true);
  });

  it("主模型快速注入实际进入同一个服务", async () => {
    const { manager, scope } = await fixture();
    const search = vi.spyOn(manager.searchService, "searchWithEvidence");
    const output = await manager.runFastKnowledgePipeline({ scope, question: "后台" });
    expect(search).toHaveBeenCalledWith(expect.objectContaining({
      channel: "fts",
      rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY,
    }));
    expect(output.stats.remoteModelCalls).toBe(0);
    expect(output.stats.injectedChunks).toBeGreaterThan(0);
    const again = await manager.runFastKnowledgePipeline({ scope, question: "后台" });
    expect(again.stats).toMatchObject({ retrievalResultCacheHit: true, ftsQueries: 0, retrievalMode: "fts", remoteModelCalls: 0 });
  });

  it("详细会话注入也通过统一服务，保留真实证据身份", async () => {
    const f = await createResearchAgentFixture(async turn => {
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const update = await turn.call("knowledge_research_update", { runId: turn.runId,
        createNeeds: [researchNeed("确认项目日期")] });
      const needId = update.needs[0].id;
      const found = await turn.call("knowledge_search", { scopeId: turn.scopeId, query: "苹果项目" });
      expect(found.isError).toBeUndefined();
      await recordSourceEvidence(turn, needId, f.sources[0].sourceId, "九月十五日");
      expect((await requestFinish(turn)).accepted).toBe(true);
    }, "请核对项目日期。");
    researchFixtures.push(f);
    const { manager } = f;
    const engine = Object.assign(Object.create(LingxiEngine.prototype) as LingxiEngine, {
      _knowledge: manager, _runtimeContext: { studioId: f.request.compiledScope.studioId },
      getSessionIdForPath: (sessionPath: string) => f.manifests.resolveByLocatorPath(sessionPath)?.sessionId ?? null,
      getSessionManifest: (sessionId: string) => f.manifests.getBySessionId(sessionId),
      executeIsolated: f.executeIsolated, emitEvent: vi.fn(),
    });
    const search = vi.spyOn(manager.searchService, "searchWithEvidence");
    expect("retrieveForNotebooks" in manager.queryService).toBe(false);
    const legacy = vi.fn(async () => { throw new Error("禁止旧入口"); });
    Object.assign(manager.queryService, { retrieveForNotebooks: legacy });
    const result = await engine.buildDetailedKnowledgeResearchContext({ question: f.request.question,
      sessionId: f.request.parentSessionId, sessionPath: f.request.parentSessionPath,
      agentId: f.request.agentId, turnId: f.request.turnId,
      knowledgeRefs: { notebookIds: f.request.compiledScope.notebookIds, mode: "detailed" } });
    expect(search).toHaveBeenCalled();
    expect(search.mock.calls.every(([request]) => request.channel === "hybrid")).toBe(true);
    expect(legacy).not.toHaveBeenCalled();
    expect(result.stats.injectedChunks).toBeGreaterThan(0);
    expect(result.evidence.entries.length).toBeGreaterThan(0);
    expect(result.stats).toMatchObject({ executionPath: "detailed_research", research: { status: "completed" } });
    const searches = f.research.listActions(result.stats.research!.runId).filter(action => action.actionType === "knowledge_search");
    expect(searches.length).toBeGreaterThan(0);
  });

});
