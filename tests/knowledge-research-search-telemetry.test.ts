import { afterEach, describe, expect, it, vi } from "vitest";
import type { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { KnowledgeQueryService } from "../lib/knowledge/knowledge-query-service.ts";
import { KnowledgeSearchService } from "../lib/knowledge/knowledge-search-service.ts";
import { KnowledgeResearchOrchestrator } from "../lib/knowledge/research/knowledge-research-orchestrator.ts";
import { createKnowledgeSearchTool, type KnowledgeSearchToolDeps } from "../lib/tools/knowledge-search-tool.ts";
import { searchToolFixture } from "./helpers/knowledge-search-tool-fixture.ts";
import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish } from "./helpers/knowledge-research-agent-fixture.ts";

type SearchSummary = Parameters<NonNullable<KnowledgeSearchToolDeps["onSearchCompleted"]>>[0];
const fixtures: Array<{ close: () => Promise<void> }> = [];
afterEach(async () => { vi.restoreAllMocks(); for (const fixture of fixtures.splice(0).reverse()) await fixture.close(); });

/** 只替换付费嵌入边界；查询、向量数据库、身份复核和搜索服务均真实执行。 */
async function installLocalVectors(manager: KnowledgeManager, studioId: string, notebookIds: string[]) {
  const modelRef = { provider: "fixture", id: "research-embedding" };
  const embed = vi.fn(async (input: { texts: string[] }) => ({
    vectors: input.texts.map(() => [1, 0]), dimensions: 2, model: { ...modelRef, api: "openai" },
  }));
  const query = new KnowledgeQueryService({ store: manager.store, indexStore: manager.indexStore,
    vectorIndex: manager.vectorIndex, embedTextsForModel: embed });
  for (const notebookId of notebookIds) {
    manager.updateNotebookSettings({ studioId, notebookId, embeddingModelRef: modelRef });
    const scope = manager.createTurnScope({ studioId, sessionPath: `/tmp/vector-preparation-${notebookId}.jsonl`, notebookIds: [notebookId] });
    try {
      const compiled = await manager.compileTurnScope(scope);
      for (const source of compiled.sources) {
        const variant = manager.indexStore.getReadyVariantMetadata({ parseArtifactId: source.parseArtifactId!,
          chunkProfileHash: compiled.notebooks[0].chunkProfileHash! })!;
        await query.embedArtifactForIngestion({ runId: "fixture-vector-index", parseArtifactId: variant.parseArtifactId,
          chunkProfileHash: variant.chunkProfileHash, embedTexts: embed });
      }
    } finally { manager.closeTurnScope({ scopeId: scope.id }); }
  }
  manager.searchService.close();
  Object.assign(manager, { searchService: new KnowledgeSearchService({ store: manager.store,
    indexStore: manager.indexStore, queryService: query }) });
  embed.mockClear();
  return { embed, vector: vi.spyOn(manager.vectorIndex, "search") };
}

function expectOnlyPublicSearchFields(payload: Record<string, unknown>, budget = false) {
  expect(Object.keys(payload).sort()).toEqual(["scopeId", "query", "mode", "vectorBackend", "citationNotice", "readingNotice", "hits", "degradedReasons", ...(budget ? ["remainingBudget"] : [])].sort());
}

describe("研究搜索的真实内部检索身份", () => {
  it("混合搜索只向宿主回传真实向量身份，公开内容和 details 保持原契约", async () => {
    const f = await searchToolFixture(); fixtures.push(f);
    const { embed, vector } = await installLocalVectors(f.manager, f.studioId, [f.notebook.id]);
    const completed = vi.fn<(summary: SearchSummary) => void>();
    const search = vi.spyOn(f.manager.searchService, "search");
    const withEvidence = vi.spyOn(f.manager.searchService, "searchWithEvidence");
    const tool = createKnowledgeSearchTool({ getKnowledge: () => f.manager, getStudioId: () => f.studioId,
      resolveSessionContext: () => f.session, resolveResearchContext: () => ({ runId: "research", actorSessionId: "root" }), onSearchCompleted: completed });
    const result = await tool.execute("research-search", f.params);
    expect(result.isError).toBeUndefined();
    expect(result.details).toEqual({ scopeId: f.scope.id });
    const payload = JSON.parse(result.content[0].text);
    expectOnlyPublicSearchFields(payload);
    expect(payload).toMatchObject({ mode: "hybrid", vectorBackend: "portable" });
    expect(payload.hits.some((hit: { channels: string[] }) => hit.channels.includes("vector"))).toBe(true);
    expect(search).not.toHaveBeenCalled(); expect(withEvidence).toHaveBeenCalledTimes(1);
    expect(embed).toHaveBeenCalledTimes(1); expect(vector.mock.calls.filter(([input]) => input.chunkIds === undefined)).toHaveLength(1);
    expect(vector.mock.calls.filter(([input]) => input.chunkIds !== undefined)).toHaveLength(1);
    const actual = await withEvidence.mock.results[0].value;
    expect(completed).toHaveBeenCalledExactlyOnceWith({ mode: "hybrid", vectorBackend: "portable",
      searchedVectorVariants: actual.evidence.searchedVectorVariants });
    const summary = completed.mock.calls[0][0];
    expect(summary.searchedVectorVariants).toHaveLength(1);
    expect(summary.searchedVectorVariants.map(item => item.vectorIndexVariantId)).toEqual(vector.mock.calls[0][0].vectorIndexVariantIds);
    expect(Object.keys(summary).sort()).toEqual(["mode", "vectorBackend", "searchedVectorVariants"].sort());
    for (const identity of summary.searchedVectorVariants) {
      expect(Object.keys(identity).sort()).toEqual(["parseArtifactId", "chunkProfileHash", "chunkIndexVariantId", "vectorIndexVariantId"].sort());
      expect(identity).toMatchObject({ parseArtifactId: payload.hits[0].parseArtifactId, chunkIndexVariantId: payload.hits[0].chunkIndexVariantId });
    }
    expect(JSON.stringify(summary)).not.toContain("年假规定");
    expect(JSON.stringify(summary)).not.toContain("snippet");
  });

  it("纯本地和配置不可用的混合请求均不伪报向量身份，非法及取消调用不通知", async () => {
    const f = await searchToolFixture(); fixtures.push(f);
    f.manager.updateNotebookSettings({ studioId: f.studioId, notebookId: f.notebook.id,
      embeddingModelRef: { provider: "unavailable", id: "not-connected" } });
    const completed = vi.fn<(summary: SearchSummary) => void>();
    const vector = vi.spyOn(f.manager.vectorIndex, "search");
    const tool = createKnowledgeSearchTool({ getKnowledge: () => f.manager, getStudioId: () => f.studioId,
      resolveSessionContext: () => f.session, onSearchCompleted: completed });
    for (const channel of ["fts", "hybrid"]) {
      const result = await tool.execute(channel, { ...f.params, channel });
      expect(result.isError).toBeUndefined();
      expect(JSON.parse(result.content[0].text).mode).toBe("fts");
      expect(completed.mock.calls.at(-1)![0]).toEqual({ mode: "fts", vectorBackend: "none", searchedVectorVariants: [] });
    }
    expect(vector).not.toHaveBeenCalled();
    const invalid = await tool.execute("invalid", { ...f.params, sourceIds: ["outside"] });
    expect(invalid.isError).toBe(true);
    await expect(tool.execute("cancelled", f.params, AbortSignal.abort())).rejects.toMatchObject({ name: "AbortError" });
    expect(completed).toHaveBeenCalledTimes(2);
  });

  it("普通工具通过带身份的搜索门面返回可引用原文", async () => {
    const f = await searchToolFixture(); fixtures.push(f);
    const search = vi.spyOn(f.manager.searchService, "searchWithEvidence");
    const result = await f.makeTool().execute("ordinary-search", f.params);
    expect(result.isError).toBeUndefined(); expect(search).toHaveBeenCalledTimes(1);
    const ordinary = JSON.parse(result.content[0].text);
    expect(ordinary.hits[0].spans[0].citationMarkdown).toContain("](");
    expect(ordinary).not.toHaveProperty("searchedVectorVariants");
    expect(result.details).toEqual({ scopeId: f.scope.id });
  });

  it("实际 Root 和 Worker 工具只通知一次，委派的真实向量身份传回宿主且不写入模型动作正文", async () => {
    const completed = vi.fn<(summary: SearchSummary) => void>();
    const f = await createResearchAgentFixture(async turn => {
      if (turn.role === "worker") {
        const need = f.research.getNeed(turn.runId, turn.options.research.allowedNeedIds[0]);
        const search = await turn.call("knowledge_search", { scopeId: turn.scopeId, query: need.ordinal === 0 ? "日期" : "预算" });
        expectOnlyPublicSearchFields(search, true); expect(search.mode).toBe("hybrid");
        await recordSourceEvidence(turn, need.id, f.sources[need.ordinal].sourceId, need.ordinal === 0 ? "九月十五日" : "三十二万元");
        return;
      }
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const search = await turn.call("knowledge_search", { scopeId: turn.scopeId, query: "苹果", channel: "fts" });
      expect(search.mode).toBe("fts");
      const created = await turn.call("knowledge_research_update", { runId: turn.runId,
        createNeeds: [researchNeed("交付日期"), researchNeed("预算金额")] });
      expect((await turn.call("knowledge_delegate", { runId: turn.runId,
        tasks: created.needs.map((need: { id: string }) => ({ label: "核对", task: "搜索后读取原文并入账", needIds: [need.id] })) })).isError).toBeUndefined();
      expect((await requestFinish(turn)).accepted).toBe(true);
    }, "核对项目进度与预算。"); fixtures.push(f);
    const { embed, vector } = await installLocalVectors(f.manager, f.request.compiledScope.studioId, f.request.compiledScope.notebookIds);
    const orchestrator = new KnowledgeResearchOrchestrator({ research: f.research,
      executeIsolated: f.executeIsolated, onSearchCompleted: completed });
    const result = await orchestrator.run(f.request);
    expect(result.run.status).toBe("completed");
    expect(f.calls.filter(turn => turn.role === "worker")).toHaveLength(2);
    expect(completed).toHaveBeenCalledTimes(3);
    expect(completed.mock.calls[0][0]).toEqual({ mode: "fts", vectorBackend: "none", searchedVectorVariants: [] });
    expect(embed).toHaveBeenCalledTimes(2); expect(vector.mock.calls.filter(([input]) => input.chunkIds === undefined)).toHaveLength(2);
    expect(vector.mock.calls.filter(([input]) => input.chunkIds !== undefined)).toHaveLength(2);
    const actualIds = [...new Set(vector.mock.calls.flatMap(([input]) => input.vectorIndexVariantIds as string[]))].sort();
    // 正文补漏允许查询冻结范围内其他来源，统计必须覆盖实际访问的全部向量身份。
    expect(actualIds).toHaveLength(3);
    for (const [summary] of completed.mock.calls.slice(1)) {
      expect(summary.mode).toBe("hybrid"); expect(summary.vectorBackend).toBe("portable");
      expect(summary.searchedVectorVariants.length).toBeGreaterThan(0);
      for (const identity of summary.searchedVectorVariants) {
        expect(actualIds).toContain(identity.vectorIndexVariantId);
        expect(f.request.compiledScope.sources.map(source => source.parseArtifactId)).toContain(identity.parseArtifactId);
      }
      expect(JSON.stringify(summary)).not.toContain("苹果项目");
    }
    const actions = f.research.listActions(result.run.id).filter(action => action.actionType === "knowledge_search");
    expect(actions).toHaveLength(3);
    expect(actions.every(action => action.status === "completed")).toBe(true);
    expect(actions.every(action => Object.keys(action.responseSummary!).sort().join() === ["count", "hitIds", "status"].sort().join())).toBe(true);
  });
});
