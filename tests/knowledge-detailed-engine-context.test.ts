import fs from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import { KnowledgeQueryService } from "../lib/knowledge/knowledge-query-service.ts";
import { KnowledgeSearchService } from "../lib/knowledge/knowledge-search-service.ts";
import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish,
  type ResearchModelTurn } from "./helpers/knowledge-research-agent-fixture.ts";

const fixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
afterEach(async () => { vi.restoreAllMocks(); for (const fixture of fixtures.splice(0).reverse()) await fixture.close(); });

async function setup(driver: (turn: ResearchModelTurn) => Promise<unknown>) {
  const f = await createResearchAgentFixture(driver, "请核对项目资料中的事实。"); fixtures.push(f);
  const executeIsolated = vi.fn(f.executeIsolated);
  const oldEntry = vi.fn(async () => { throw new Error("新的详细入口不得使用旧调查编排"); });
  const auxiliary = vi.fn(async () => { throw new Error("新的详细入口不得使用旧辅助模型"); });
  const engine = Object.assign(Object.create(LingxiEngine.prototype) as LingxiEngine, {
    _knowledge: f.manager, _runtimeContext: { studioId: f.request.compiledScope.studioId },
    getSessionIdForPath: (sessionPath: string) => f.manifests.resolveByLocatorPath(sessionPath)?.sessionId ?? null,
    getSessionManifest: (sessionId: string) => f.manifests.getBySessionId(sessionId),
    executeIsolated, buildKnowledgeContextInjection: oldEntry,
    resolveAuxiliaryExecution: auxiliary, resolveAuxiliaryExecutionFresh: auxiliary,
    resolveAuxiliaryModel: auxiliary, resolveAuxiliaryModelFresh: auxiliary,
  });
  const input = { question: f.request.question,
    knowledgeRefs: { notebookIds: f.request.compiledScope.notebookIds, mode: "detailed" as const },
    sessionId: f.request.parentSessionId, sessionPath: f.request.parentSessionPath,
    agentId: f.request.agentId, turnId: f.request.turnId };
  return { ...f, engine, input, executeIsolated, oldEntry, auxiliary };
}

async function startNeed(turn: ResearchModelTurn, claim = "确认项目日期") {
  expect((await turn.call("knowledge_outline", { scopeId: turn.scopeId })).isError).toBeUndefined();
  const update = await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed(claim)] });
  expect(update.isError).toBeUndefined();
  return update.needs[0].id as string;
}

async function grepEvidence(turn: ResearchModelTurn, needId: string, sourceId: string, quote: string) {
  const found = await turn.call("knowledge_grep", { scopeId: turn.scopeId, pattern: quote, sourceIds: [sourceId] });
  expect(found.isError).toBeUndefined();
  expect(found.matches).toHaveLength(1);
  const receiptId = found.matches[0].receiptId as string;
  const update = await turn.call("knowledge_research_update", { runId: turn.runId,
    linkEvidence: [{ needId, receiptId, quote, relation: "supports", rationale: "冻结原文明示" }] });
  expect(update.isError).toBeUndefined();
  return receiptId;
}

describe("Engine 真实详细研究上下文", () => {
  it("只执行隔离研究，真实阅读后同分块的多个精确引文都进入最终持久化清单", async () => {
    const quotes = ["苹果项目", "九月十五日"];
    const f = await setup(async turn => {
      expect(turn.role).toBe("root");
      const needId = await startNeed(turn);
      const read = await turn.call("knowledge_read", { scopeId: turn.scopeId, sourceId: f.sources[0].sourceId });
      expect(read.isError).toBeUndefined();
      const receiptId = read.chunks[0].spans[0].receiptId;
      expect((await turn.call("knowledge_research_update", { runId: turn.runId,
        linkEvidence: quotes.map(quote => ({ needId, receiptId, quote, relation: "supports", rationale: "原文定位" })) })).isError).toBeUndefined();
      expect((await requestFinish(turn)).accepted).toBe(true);
    });
    const result = await f.engine.buildDetailedKnowledgeResearchContext(f.input);
    expect(f.executeIsolated).toHaveBeenCalledTimes(1);
    expect(f.executeIsolated.mock.calls[0][1]).toMatchObject({ surface: "knowledge_research_root", agentId: f.input.agentId,
      parentSessionId: f.input.sessionId, parentSessionPath: f.input.sessionPath });
    expect(f.oldEntry).not.toHaveBeenCalled(); expect(f.auxiliary).not.toHaveBeenCalled();
    expect(result.stats).toMatchObject({ mode: "detailed", executionPath: "detailed_research", readCalls: 1,
      injectedChunks: 2, research: { status: "completed", rounds: 1, needsSupported: 1 } });
    expect(result.block).toContain("[KnowledgeResearchContext]");
    expect(result.block).not.toContain("私有模型推理");
    expect(result.evidence.entries).toHaveLength(1);
    const entry = result.evidence.entries[0];
    expect([...entry.citationLabels].sort()).toEqual(["K1", "K2"]);
    expect(entry.blockSpans).toHaveLength(2);
    const chunk = f.manager.indexStore.listVariantChunks(entry.chunkIndexVariantId).find(item => item.id === entry.chunkId)!;
    const actualQuotes = entry.blockSpans.map(span => {
      expect(span.blockEndOffset - span.blockStartOffset).toBe(span.chunkEndOffset - span.chunkStartOffset);
      const original = f.sources[0].text.slice(span.blockStartOffset, span.blockEndOffset);
      expect(chunk.text.slice(span.chunkStartOffset, span.chunkEndOffset)).toBe(original);
      return original;
    });
    expect(actualQuotes.sort()).toEqual([...quotes].sort());
    f.engine.recordKnowledgeEvidenceManifest({ sessionPath: f.input.sessionPath, stats: result.stats, evidence: result.evidence });
    const manifest = f.manager.store.getEvidenceManifestByScope({ scopeId: result.stats.scopeId })!;
    expect(manifest.entries).toHaveLength(1);
    expect(manifest.entries[0].blockSpans).toEqual([{ chunkId: chunk.id, spans: entry.blockSpans }]);
    expect([...manifest.entries[0].citationLabels].sort()).toEqual(["K1", "K2"]);
    const receipts = f.manager.store.db.prepare("SELECT consumed_at FROM knowledge_research_read_receipts WHERE run_id = ?").all(result.stats.research!.runId);
    expect(receipts).toHaveLength(1); expect(receipts[0].consumed_at).not.toBeNull();
    expect(f.sessionPaths.every(file => !fs.existsSync(file))).toBe(true);
  });

  it("grep 的无分块凭据按真实索引补齐身份，最终清单只定位精确引文", async () => {
    const quote = "三十二万元";
    let receiptId = "";
    const f = await setup(async turn => {
      const needId = await startNeed(turn, "确认项目预算");
      receiptId = await grepEvidence(turn, needId, f.sources[1].sourceId, quote);
      expect((await requestFinish(turn)).accepted).toBe(true);
    });
    const result = await f.engine.buildDetailedKnowledgeResearchContext(f.input);
    const receipt = f.research.getReceipt(result.stats.research!.runId, receiptId);
    expect(receipt).toMatchObject({ chunkId: null, chunkIndexVariantId: null, channel: "knowledge_grep" });
    expect(receipt.consumedAt).not.toBeNull();
    expect(f.research.listEvidence(result.stats.research!.runId)[0].chunkId).toBeNull();
    expect(result.stats).toMatchObject({ grepCalls: 1, readCalls: 0, retrievalMode: "none", injectedChunks: 1 });
    const entry = result.evidence.entries[0];
    const chunk = f.manager.indexStore.listVariantChunks(entry.chunkIndexVariantId).find(item => item.id === entry.chunkId)!;
    const start = f.sources[1].text.indexOf(quote);
    expect(entry.blockSpans).toEqual([expect.objectContaining({ blockId: receipt.blockId, blockStartOffset: start, blockEndOffset: start + quote.length })]);
    expect(chunk.text.slice(entry.blockSpans[0].chunkStartOffset, entry.blockSpans[0].chunkEndOffset)).toBe(quote);
    expect(entry.citationLabels).toEqual(["K1"]);
    f.engine.recordKnowledgeEvidenceManifest({ sessionPath: f.input.sessionPath, stats: result.stats, evidence: result.evidence });
    expect(f.manager.store.getEvidenceManifestByScope({ scopeId: result.stats.scopeId })!.entries[0].chunkIds).toEqual([chunk.id]);
  });

  it("真实混合搜索的向量身份去重后经过 Engine 上下文进入持久化证据清单", async () => {
    const f = await setup(async turn => {
      const needId = await startNeed(turn);
      for (const query of ["日期", "交付"]) {
        const search = await turn.call("knowledge_search", { scopeId: turn.scopeId, query,
          sourceIds: [f.sources[0].sourceId], channel: "hybrid" });
        expect(search.isError).toBeUndefined();
        expect(search.mode).toBe("hybrid");
        expect(search.hits.some((hit: { channels: string[] }) => hit.channels.includes("vector"))).toBe(true);
      }
      await recordSourceEvidence(turn, needId, f.sources[0].sourceId, "九月十五日");
      expect((await requestFinish(turn)).accepted).toBe(true);
    });
    const studioId = f.request.compiledScope.studioId, notebook = f.request.compiledScope.notebooks[0];
    const modelRef = { provider: "fixture", id: "engine-research-embedding" };
    f.manager.updateNotebookSettings({ studioId, notebookId: notebook.notebookId, embeddingModelRef: modelRef });
    // 只替换付费嵌入边界；真实向量数据库、搜索服务、Engine 和持久化清单全部贯通。
    const embed = vi.fn(async (input: { texts: string[] }) => ({ vectors: input.texts.map(() => [1, 0]),
      dimensions: 2, model: { ...modelRef, api: "openai" } }));
    const query = new KnowledgeQueryService({ store: f.manager.store, indexStore: f.manager.indexStore,
      vectorIndex: f.manager.vectorIndex, embedTextsForModel: embed });
    for (const source of f.request.compiledScope.sources) {
      await query.embedArtifactForIngestion({ runId: "fixture-vector-index", parseArtifactId: source.parseArtifactId!,
        chunkProfileHash: notebook.chunkProfileHash!, embedTexts: embed });
    }
    f.manager.searchService.close();
    Object.assign(f.manager, { searchService: new KnowledgeSearchService({ store: f.manager.store,
      indexStore: f.manager.indexStore, queryService: query }) });
    embed.mockClear();
    const vector = vi.spyOn(f.manager.vectorIndex, "search");
    const result = await f.engine.buildDetailedKnowledgeResearchContext(f.input);
    expect(embed).toHaveBeenCalledTimes(2); expect(vector).toHaveBeenCalledTimes(2);
    const actualIds = [...new Set(vector.mock.calls.flatMap(([input]) => input.vectorIndexVariantIds as string[]))];
    expect(actualIds).toHaveLength(1);
    expect(result.stats).toMatchObject({ retrievalMode: "hybrid", vectorBackend: "portable", searchCalls: 2,
      research: { status: "completed" } });
    const source = f.request.compiledScope.sources.find(item => item.sourceId === f.sources[0].sourceId)!;
    expect(result.evidence.searchedVectorVariants).toEqual([{ parseArtifactId: source.parseArtifactId,
      chunkProfileHash: notebook.chunkProfileHash, chunkIndexVariantId: source.chunkIndexVariantId,
      vectorIndexVariantId: actualIds[0] }]);
    f.engine.recordKnowledgeEvidenceManifest({ sessionPath: f.input.sessionPath, stats: result.stats, evidence: result.evidence });
    const manifest = f.manager.store.getEvidenceManifestByScope({ scopeId: result.stats.scopeId })!;
    expect(manifest.entries).toHaveLength(1);
    expect(manifest.entries[0]).toMatchObject({ sourceId: source.sourceId, parseArtifactId: source.parseArtifactId,
      chunkIndexVariantId: source.chunkIndexVariantId, vectorIndexVariantIds: actualIds, citationLabels: ["K1"] });
    expect(f.oldEntry).not.toHaveBeenCalled(); expect(f.auxiliary).not.toHaveBeenCalled();
  });

  it.each(["missing", "incomplete"] as const)("原文已核验但索引 %s 时明确失败，保留研究台账而不伪造或丢弃引文", async mode => {
    let runId = "", scopeId = "";
    const quote = "三十二万元";
    const f = await setup(async turn => {
      runId = turn.runId; scopeId = turn.scopeId;
      const needId = await startNeed(turn, "确认项目预算");
      await grepEvidence(turn, needId, f.sources[1].sourceId, quote);
      expect((await requestFinish(turn)).accepted).toBe(true);
      const source = f.manager.getTurnScope({ scopeId })!.sources.find(item => item.sourceId === f.sources[1].sourceId)!;
      expect(f.manager.store.getParseArtifact({ studioId: f.request.compiledScope.studioId, parseArtifactId: source.parseArtifactId! }).status).toBe("ready");
      if (mode === "missing") f.manager.indexStore.removeArtifact(source.parseArtifactId!);
      else {
        const variant = f.request.compiledScope.sources.find(item => item.sourceId === source.sourceId)!.chunkIndexVariantId!;
        const chunk = f.manager.indexStore.listVariantChunks(variant)[0];
        const end = f.sources[1].text.indexOf(quote) + quote.length - 1;
        const spans = chunk.spans.map(span => ({ ...span, blockEndOffset: end,
          chunkEndOffset: span.chunkStartOffset + end - span.blockStartOffset }));
        f.manager.indexStore.db.prepare("UPDATE knowledge_chunks SET spans_json = ? WHERE id = ?").run(JSON.stringify(spans), chunk.id);
      }
    });
    await expect(f.engine.buildDetailedKnowledgeResearchContext(f.input)).rejects.toMatchObject({ code: "KNOWLEDGE_INDEX_INVALID" });
    expect(f.research.listEvidence(runId).map(item => item.canonicalText)).toEqual([quote]);
    expect(f.manager.store.getEvidenceManifestByScope({ scopeId })).toBeNull();
    expect(f.oldEntry).not.toHaveBeenCalled(); expect(f.auxiliary).not.toHaveBeenCalled();
  });

  it("实际 Worker 搜索方式和计数传回详细统计，Root 未搜索也不能记成 none", async () => {
    const f = await setup(async turn => {
      if (turn.role === "worker") {
        const need = f.research.getNeed(turn.runId, turn.options.research.allowedNeedIds[0]);
        const search = await turn.call("knowledge_search", { scopeId: turn.scopeId,
          query: need.ordinal === 0 ? "日期" : "预算", channel: "fts" });
        expect(search.mode).toBe("fts"); expect(search.hits.length).toBeGreaterThan(0);
        await recordSourceEvidence(turn, need.id, f.sources[need.ordinal].sourceId, need.ordinal === 0 ? "九月十五日" : "三十二万元");
        return;
      }
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const needs = (await turn.call("knowledge_research_update", { runId: turn.runId,
        createNeeds: [researchNeed("日期"), researchNeed("预算")] })).needs as Array<{ id: string }>;
      expect((await turn.call("knowledge_delegate", { runId: turn.runId,
        tasks: needs.map(need => ({ label: "查证", task: "先检索再读取原文入账", needIds: [need.id] })) })).isError).toBeUndefined();
      expect((await requestFinish(turn)).accepted).toBe(true);
    });
    const result = await f.engine.buildDetailedKnowledgeResearchContext(f.input);
    expect(f.calls.filter(turn => turn.role === "worker")).toHaveLength(2);
    expect(result.stats).toMatchObject({ retrievalMode: "fts", vectorBackend: "none", searchCalls: 2, readCalls: 2,
      research: { status: "completed", delegatedAgents: 2, needsSupported: 2 } });
    expect(result.stats.subQueries.sort()).toEqual(["日期", "预算"].sort());
    expect(result.stats.subQueryHits.every(count => count > 0)).toBe(true);
    expect(result.evidence.entries).toHaveLength(2);
    expect(f.oldEntry).not.toHaveBeenCalled(); expect(f.auxiliary).not.toHaveBeenCalled();
  });

  it.each(["cancelled", "failed"] as const)("研究 %s 时不返回成功上下文，隔离会话完成清理", async status => {
    const controller = new AbortController();
    let runId = "";
    const f = await setup(async turn => {
      runId = turn.runId;
      if (status === "failed") throw new Error("私有供应商失败正文");
      const needId = await startNeed(turn);
      await recordSourceEvidence(turn, needId, f.sources[0].sourceId, "九月十五日");
      controller.abort();
    });
    const pending = f.engine.buildDetailedKnowledgeResearchContext({ ...f.input, signal: controller.signal });
    if (status === "cancelled") await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    else await expect(pending).rejects.toMatchObject({ code: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE" });
    expect(f.research.requireRun(runId).status).toBe(status);
    expect(f.sessionPaths.every(file => !fs.existsSync(file))).toBe(true);
    expect(f.oldEntry).not.toHaveBeenCalled(); expect(f.auxiliary).not.toHaveBeenCalled();
  });

  it("会话路径、会话编号或 Agent 身份不符时，在创建研究与执行模型之前拒绝", async () => {
    const f = await setup(async () => { throw new Error("错误身份不应运行模型"); });
    for (const changed of [{ sessionPath: `${f.input.sessionPath}.other` }, { sessionId: "another-session" }, { agentId: "agent-b" }]) {
      await expect(f.engine.buildDetailedKnowledgeResearchContext({ ...f.input, ...changed })).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    }
    expect(f.executeIsolated).not.toHaveBeenCalled();
    expect(f.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_runs").get().count).toBe(0);
  });
});
