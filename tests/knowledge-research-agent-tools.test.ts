import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Agent } from "../core/agent.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { ResearchStore } from "../lib/knowledge/research/research-store.ts";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { KnowledgeError } from "../lib/knowledge/errors.ts";
import { resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";
import { getKnowledgeResearchToolNames } from "../shared/tool-categories.ts";
import type { KnowledgeResearchActorContext } from "../lib/knowledge/research/research-tool-budget.ts";

const cleanup: Array<() => Promise<void>> = [];
afterEach(async () => { for (const close of cleanup.splice(0)) await close(); });
async function fixture(withSections = false) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-research-agent-"));
  const manager = new KnowledgeManager({ lingxiHome: root });
  const manifests = new SessionManifestStore({ dbPath: path.join(root, "manifests.db") });
  cleanup.push(async () => { await manager.close(); manifests.close(); fs.rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); });
  const studioId = "research-agent-studio";
  const notebook = manager.createNotebook({ studioId, name: "项目资料" });
  const sources = [];
  for (const [name, text] of [["进度", "苹果项目交付日期九月十五日。"], ["预算", "苹果项目预算三十二万元。"]]) {
    const imported = await manager.importPastedText({ studioId, notebookId: notebook.id, displayName: name, text });
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    if (withSections && name === "进度") {
      manager.store.completeParseArtifact({ studioId, parseArtifactId: artifact.id, status: "ready", warnings: [],
        semanticArtifactPath: `artifacts/${artifact.id}.json`, blocks: [
          { ordinal: 0, locatorType: "text", text, locator: { headingPath: ["第一章"], lineNumber: 1 } },
          { ordinal: 1, locatorType: "text", text: "苹果项目进展顺利。", locator: { headingPath: ["第二章"], lineNumber: 2 } },
        ] });
    }
    const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
    const targetChars = manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id });
    manager.store.resolveNotebookRetrievalProfile({ studioId, notebookId: notebook.id,
      strategy: resolveKnowledgeChunkerConfig(blocks, { targetChars }).strategy });
    manager.queryService.indexArtifactForIngestion(studioId, artifact.id, { targetChars });
    sources.push({ sourceId: imported.source.id, text });
  }
  function manifest(name: string, kind: string, parentSessionId?: string, researchContext?: Record<string, unknown>) {
    const sessionPath = path.join(root, `${name}.jsonl`); fs.writeFileSync(sessionPath, "");
    return manifests.createForPath({ sessionPath, ownerAgentId: "agent-a", domain: kind === "chat" ? "desktop" : "subagent", kind,
      provenance: { ...(parentSessionId ? { parentSessionId } : {}), studioId, ...(researchContext ? { researchContext } : {}) } });
  }
  const main = manifest("main", "chat");
  const scope = manager.createTurnScope({ studioId, sessionPath: main.currentLocator.path, notebookIds: [notebook.id] });
  const research = new ResearchStore(manager.store);
  const run = research.createRun({ turnScopeId: scope.id, turnId: scope.turnId, parentSessionPath: scope.sessionPath, question: "苹果项目进度和预算是什么？" });
  const need = research.createNeed(run.id, { claim: "确定项目事实", kind: "fact", required: true,
    minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false });
  const rootManifest = manifest("research-root", "knowledge_research_root", main.sessionId, { runId: run.id, scopeId: scope.id, role: "root" });
  const workerManifest = manifest("research-worker", "knowledge_research_worker", rootManifest.sessionId,
    { runId: run.id, scopeId: scope.id, role: "worker", allowedNeedIds: [need.id], allowedSourceIds: [sources[0].sourceId] });
  const engine = { knowledge: manager, runtimeContext: { studioId },
    getSessionIdForPath: (sessionPath: string) => manifests.resolveByLocatorPath(sessionPath)?.sessionId ?? null,
    getSessionManifest: (sessionId: string) => manifests.getBySessionId(sessionId) };
  const executeIsolated = vi.fn().mockResolvedValue({ replyText: "不应回显的工作会话原始输出" });
  const agent = Object.assign(Object.create(Agent.prototype) as Agent, {
    _cb: { getEngine: () => engine, listActiveAgents: () => [{ id: "agent-a" }], executeIsolated },
  });
  const context = (worker = false): KnowledgeResearchActorContext => ({ runId: run.id, scopeId: scope.id,
    actorSessionId: (worker ? workerManifest : rootManifest).sessionId, actorAgentId: "agent-a", role: worker ? "worker" : "root",
    ...(worker ? { allowedNeedIds: [need.id], allowedSourceIds: [sources[0].sourceId] } : {}) });
  const snapshot = (worker = false, planning: { searchPlan?: Array<{ query: string; needIds: string[]; purpose?: "counterexample" }>;
    forbiddenQueries?: string[] } = {}) => agent.getToolsSnapshot({ surface: worker ? "knowledge_research_worker" : "knowledge_research_root",
    research: { runId: run.id, scopeId: scope.id, studioId, actorContext: context(worker),
      sessionPath: (worker ? workerManifest : rootManifest).currentLocator.path, ...planning } });
  const runtime = (worker = false) => ({ sessionManager: { getSessionFile: () => (worker ? workerManifest : rootManifest).currentLocator.path } });
  return { manager, research, run, scope, need, sources, rootManifest, workerManifest, manifests, agent, snapshot, runtime, executeIsolated };
}

describe("真实 Agent 研究工具快照", () => {
  it("主研究仅七种、工作会话仅五种工具，缺宿主研究绑定拒绝装配", async () => {
    const f = await fixture();
    expect(f.snapshot().map(tool => tool.name)).toEqual([...getKnowledgeResearchToolNames("knowledge_research_root")]);
    expect(f.snapshot(true).map(tool => tool.name)).toEqual([...getKnowledgeResearchToolNames("knowledge_research_worker")]);
    expect(() => f.agent.getToolsSnapshot({ surface: "knowledge_research_root" })).toThrow(/bound/);
  });

  it("主研究实际本地搜索、工作会话实际原文阅读和入账，共用调用与凭据记录", async () => {
    const f = await fixture();
    const search = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    const result = await search.execute("search", { scopeId: f.scope.id, query: "苹果", channel: "fts" }, undefined, undefined, f.runtime());
    expect(result.isError).toBeFalsy();
    expect(JSON.parse(result.content[0].text).hits.length).toBeGreaterThan(0);
    const read = f.snapshot(true).find(tool => tool.name === "knowledge_read")!;
    const raw = await read.execute("read", { scopeId: f.scope.id, sourceId: f.sources[0].sourceId }, undefined, undefined, f.runtime(true));
    expect(raw.isError).toBeFalsy();
    const span = JSON.parse(raw.content[0].text).chunks[0].spans[0];
    expect(span.text).toContain("九月十五日");
    const update = f.snapshot(true).find(tool => tool.name === "knowledge_research_update")!;
    const linked = await update.execute("update", { runId: f.run.id, linkEvidence: [{ needId: f.need.id,
      receiptId: span.receiptId, quote: "九月十五日", relation: "supports", rationale: "原文给出日期" }] }, undefined, undefined, f.runtime(true));
    expect(linked.isError).toBeFalsy();
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 3, searchCalls: 1, readCalls: 1 });
    expect(f.research.listEvidence(f.run.id)[0].canonicalText).toBe("九月十五日");
    const actions = f.research.listActions(f.run.id);
    expect(actions[0].responseSummary?.hitIds).not.toHaveLength(0);
    expect(actions[1].responseSummary?.receiptIds).toContain(span.receiptId);
    expect(JSON.stringify(actions)).not.toContain(f.sources[0].text);
  });

  it("非法搜索参数保留参数错误并计数，不能误报为关键检索不可用", async () => {
    const f = await fixture();
    const query = vi.spyOn(f.manager.searchService, "search");
    const search = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    const result = await search.execute("invalid-limit", { scopeId: f.scope.id, query: "苹果", channel: "fts", limit: 0 },
      undefined, undefined, f.runtime());
    expect(result.isError).toBe(true);
    expect(result.details.errorCode).toBe("KNOWLEDGE_INVALID_ARGUMENT");
    expect(query).not.toHaveBeenCalled();
    expect(f.research.requireRun(f.run.id)).toMatchObject({ status: "planning", toolCallsUsed: 1, searchCalls: 1 });
    expect(f.research.listActions(f.run.id)).toMatchObject([{ status: "failed", errorCode: "KNOWLEDGE_INVALID_ARGUMENT" }]);
    expect((await search.execute("fixed-limit", { scopeId: f.scope.id, query: "苹果", channel: "fts", limit: 1 },
      undefined, undefined, f.runtime())).isError).toBeFalsy();
    expect(query).toHaveBeenCalledOnce();
  });

  it("只保留契约内错误码，未知字符串不能经返回值或动作记录外逸", async () => {
    const f = await fixture();
    const privateCode = "KNOWLEDGE_PRIVATE_error_contains_raw_document";
    const unknown = Object.assign(new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "私有底层错误原文"), { code: privateCode });
    const query = vi.spyOn(f.manager.searchService, "search")
      .mockRejectedValueOnce(new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "私有底层错误原文"))
      .mockRejectedValueOnce(unknown);
    const search = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    const known = await search.execute("known-error", { scopeId: f.scope.id, query: "苹果", channel: "fts" },
      undefined, undefined, f.runtime());
    const unrecognized = await search.execute("unknown-error", { scopeId: f.scope.id, query: "苹果", channel: "fts" },
      undefined, undefined, f.runtime());
    expect(known.details.errorCode).toBe("KNOWLEDGE_INDEX_INVALID");
    expect(unrecognized.details.errorCode).toBe("KNOWLEDGE_RETRIEVAL_UNAVAILABLE");
    expect(f.research.listActions(f.run.id).map(action => action.errorCode))
      .toEqual(["KNOWLEDGE_INDEX_INVALID", "KNOWLEDGE_RETRIEVAL_UNAVAILABLE"]);
    const exposed = JSON.stringify({ known, unrecognized, actions: f.research.listActions(f.run.id) });
    expect(exposed).not.toContain(privateCode);
    expect(exposed).not.toContain("私有底层错误原文");
    expect(query).toHaveBeenCalledTimes(2);
  });

  it("工作会话检索和目录默认限制来源子集，显式扩大来源整体拒绝", async () => {
    const f = await fixture(); const tools = f.snapshot(true);
    const search = tools.find(tool => tool.name === "knowledge_search")!;
    const found = await search.execute("search", { scopeId: f.scope.id, query: "苹果", channel: "fts" }, undefined, undefined, f.runtime(true));
    expect(found.isError).toBeFalsy();
    const hits = JSON.parse(found.content[0].text).hits;
    expect(hits.length).toBeGreaterThan(0);
    expect(hits.every((hit: { sourceId: string }) => hit.sourceId === f.sources[0].sourceId)).toBe(true);
    const outside = await search.execute("outside", { scopeId: f.scope.id, query: "苹果", channel: "fts", sourceIds: [f.sources[1].sourceId] }, undefined, undefined, f.runtime(true));
    expect(outside.isError).toBe(true);
    const outline = tools.find(tool => tool.name === "knowledge_outline")!;
    const listed = await outline.execute("outline", { scopeId: f.scope.id }, undefined, undefined, f.runtime(true));
    expect(listed.isError).toBeFalsy();
    const tree = JSON.parse(listed.content[0].text);
    expect(tree.totalSources).toBe(1);
    expect(JSON.stringify(tree)).not.toContain(f.sources[1].sourceId);
  });

  it("工具对象不能被另一个会话借用，清单中的权限范围改动会使旧对象失效", async () => {
    const f = await fixture();
    const tool = f.snapshot(true).find(tool => tool.name === "knowledge_read")!;
    const params = { scopeId: f.scope.id, sourceId: f.sources[0].sourceId };
    expect((await tool.execute("stolen", params, undefined, undefined, f.runtime())).isError).toBe(true);
    const provenance = { ...f.workerManifest.provenance,
      researchContext: { ...f.workerManifest.provenance.researchContext, allowedSourceIds: [] } };
    f.manifests.db.prepare("UPDATE session_manifests SET provenance_json=? WHERE session_id=?").run(JSON.stringify(provenance), f.workerManifest.sessionId);
    expect((await tool.execute("drift", params, undefined, undefined, f.runtime(true))).isError).toBe(true);
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(0);
  });

  it("研究登记仍相同但祖先转到别的主会话时，更新和完成也必须拒绝", async () => {
    const f = await fixture(); const tools = f.snapshot();
    const otherPath = path.join(path.dirname(f.rootManifest.currentLocator.path), "another-main.jsonl");
    fs.writeFileSync(otherPath, "");
    const other = f.manifests.createForPath({ sessionPath: otherPath, ownerAgentId: "agent-a", domain: "desktop", kind: "chat", provenance: {} });
    f.manifests.db.prepare("UPDATE session_manifests SET provenance_json=? WHERE session_id=?")
      .run(JSON.stringify({ ...f.rootManifest.provenance, parentSessionId: other.sessionId }), f.rootManifest.sessionId);
    const update = tools.find(tool => tool.name === "knowledge_research_update")!;
    const result = await update.execute("wrong-parent", { runId: f.run.id, unresolvedGaps: [{ needId: f.need.id, gaps: ["未核查"] }] }, undefined, undefined, f.runtime());
    expect(result.isError).toBe(true);
    const finish = tools.find(tool => tool.name === "knowledge_research_finish")!;
    expect((await finish.execute("wrong-parent-finish", { runId: f.run.id, conclusionSummary: "完成", requestedStopReason: "complete" }, undefined, undefined, f.runtime())).isError).toBe(true);
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(0);
    expect(f.research.getNeed(f.run.id, f.need.id).unresolvedGaps).toEqual([]);
  });

  it("主研究委派时以真实Root为父级，沿两层回到主会话且不回显工作会话正文", async () => {
    const f = await fixture(); const delegate = f.snapshot().find(tool => tool.name === "knowledge_delegate")!;
    const result = await delegate.execute("delegate", { runId: f.run.id, tasks: [{ label: "查日期", needIds: [f.need.id], task: "找日期的原文证据" }] }, undefined, undefined, f.runtime());
    expect(result.isError).toBeFalsy();
    expect(f.executeIsolated).toHaveBeenCalledOnce();
    const options = f.executeIsolated.mock.calls[0][1];
    expect(options.parentSessionId).toBe(f.rootManifest.sessionId);
    expect(options.parentSessionPath).toBe(f.rootManifest.currentLocator.path);
    expect(options.research).toMatchObject({ runId: f.run.id, scopeId: f.scope.id, allowedNeedIds: [f.need.id] });
    expect(JSON.stringify(result)).not.toContain("不应回显的工作会话原始输出");
  });

  it("只有精确匹配宿主计划的实际查询才能登记反证检查，支持宽窄字符、空白和大小写归一", async () => {
    const f = await fixture();
    const plan = [{ query: "ＦＯＯ\tＢＡＲ", needIds: [f.need.id], purpose: "counterexample" as const }];
    const search = f.snapshot(false, { searchPlan: plan }).find(tool => tool.name === "knowledge_search")!;
    plan[0].needIds.length = 0;
    const result = await search.execute("counter-search", { scopeId: f.scope.id, query: "  foo   bar  ", channel: "fts" },
      undefined, undefined, f.runtime());
    expect(result.isError).toBeFalsy();
    expect(JSON.parse(result.content[0].text).hits).toEqual([]);
    expect(f.research.listActions(f.run.id)[0]).toMatchObject({ status: "completed",
      requestSummary: { query: "  foo   bar  ", needIds: [f.need.id], purpose: "counterexample" },
      responseSummary: { count: 0, hitIds: [] } });
    expect([...(f.research.listActions(f.run.id)[0].requestSummary.sourceIds as string[])].sort())
      .toEqual(f.sources.map(source => source.sourceId).sort());
    expect(new EvidenceLedger(f.research).evaluateNeed(f.run.id, f.need.id).counterEvidenceChecked).toBe(true);
    expect(f.manifests.getBySessionId(f.rootManifest.sessionId)?.provenance).not.toHaveProperty("searchPlan");
  });

  it("普通搜索和伪造运行上下文不产生反证标记，锁定工具参数仍不接受purpose或needIds", async () => {
    const f = await fixture(); const search = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    expect(Object.keys(search.parameters.properties)).not.toContain("purpose");
    expect(Object.keys(search.parameters.properties)).not.toContain("needIds");
    const result = await search.execute("ordinary", { scopeId: f.scope.id, query: "counterexample none", channel: "fts" },
      undefined, undefined, { ...f.runtime(), searchPlan: [{ query: "counterexample none", needIds: [f.need.id], purpose: "counterexample" }] });
    expect(result.isError).toBeFalsy();
    expect(f.research.listActions(f.run.id)[0].requestSummary).not.toHaveProperty("purpose");
    expect(new EvidenceLedger(f.research).evaluateNeed(f.run.id, f.need.id).counterEvidenceChecked).toBe(false);
    const forged = await search.execute("forged-params", { scopeId: f.scope.id, query: "forged counter", channel: "fts",
      purpose: "counterexample", needIds: [f.need.id] }, undefined, undefined, f.runtime());
    expect(forged.isError).toBe(true);
    expect(f.research.listActions(f.run.id).at(-1)?.requestSummary).not.toHaveProperty("purpose");
  });

  it("同一计划查询兼有普通和反证用途时不能把普通调查升级成反证检查", async () => {
    const f = await fixture();
    const search = f.snapshot(false, { searchPlan: [{ query: "absent", needIds: [f.need.id] },
      { query: "ABSENT", needIds: [f.need.id], purpose: "counterexample" }] }).find(tool => tool.name === "knowledge_search")!;
    expect((await search.execute("mixed-purpose", { scopeId: f.scope.id, query: "absent", channel: "fts" }, undefined, undefined, f.runtime())).isError).toBeFalsy();
    expect(f.research.listActions(f.run.id)[0].requestSummary).toMatchObject({ needIds: [f.need.id] });
    expect(f.research.listActions(f.run.id)[0].requestSummary).not.toHaveProperty("purpose");
  });

  it("空来源或空章节的零命中不能冒充宿主反证调查", async () => {
    const f = await fixture();
    const search = f.snapshot(false, { searchPlan: [{ query: "absent sources", needIds: [f.need.id], purpose: "counterexample" },
      { query: "absent sections", needIds: [f.need.id], purpose: "counterexample" }] }).find(tool => tool.name === "knowledge_search")!;
    for (const params of [{ query: "absent sources", sourceIds: [] }, { query: "absent sections", sectionKeys: [] }]) {
      const result = await search.execute("empty", { scopeId: f.scope.id, channel: "fts", ...params }, undefined, undefined, f.runtime());
      expect(result.isError).toBeFalsy();
      expect(f.research.listActions(f.run.id).at(-1)?.requestSummary).not.toHaveProperty("purpose");
    }
    expect(new EvidenceLedger(f.research).evaluateNeed(f.run.id, f.need.id).counterEvidenceChecked).toBe(false);
  });

  it("规范化等价查询在调用检索服务前拒绝，换快照不能重置成功历史", async () => {
    const f = await fixture(); const executeSearch = vi.spyOn(f.manager.searchService, "search");
    const first = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    expect((await first.execute("first", { scopeId: f.scope.id, query: "apple   project", channel: "fts" }, undefined, undefined, f.runtime())).isError).toBeFalsy();
    const second = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    const repeated = await second.execute("repeated", { scopeId: f.scope.id, query: " ＡＰＰＬＥ\nＰＲＯＪＥＣＴ ", channel: "fts" }, undefined, undefined, f.runtime());
    expect(repeated.details.errorCode).toBe("KNOWLEDGE_CONFLICT");
    expect(executeSearch).toHaveBeenCalledOnce();
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(2);
    expect(f.research.listActions(f.run.id).at(-1)?.status).toBe("failed");
  });

  it("同源不同章节和整源允许分别调查，同章节集合跨快照归一后仍拒绝重复", async () => {
    const f = await fixture(true);
    const compiled = await f.manager.compileTurnScope(f.scope);
    const sections = compiled.sources.find(source => source.sourceId === f.sources[0].sourceId)!.sectionKeys;
    expect(sections).toHaveLength(2);
    const query = vi.spyOn(f.manager.searchService, "search");
    const params = { scopeId: f.scope.id, query: "苹果", channel: "fts", sourceIds: [f.sources[0].sourceId] };
    const search = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    for (const sectionKeys of [undefined, [sections[0]], [sections[1]], sections]) {
      expect((await search.execute("new-section", { ...params, ...(sectionKeys ? { sectionKeys } : {}) },
        undefined, undefined, f.runtime())).isError).toBeFalsy();
    }
    expect(f.research.listActions(f.run.id).map(action => action.requestSummary.sectionKeys))
      .toEqual([undefined, [sections[0]], [sections[1]], [...sections].sort()]);
    const nextSnapshot = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    const duplicate = await nextSnapshot.execute("same-sections", { ...params, sectionKeys: [sections[1], sections[0], sections[1]] },
      undefined, undefined, f.runtime());
    expect(duplicate.details.errorCode).toBe("KNOWLEDGE_CONFLICT");
    expect(query).toHaveBeenCalledTimes(4);
  });

  it("整源字符串禁表不封锁章节过滤，非法章节参数仍按原工具报错并计数", async () => {
    const f = await fixture(true);
    const compiled = await f.manager.compileTurnScope(f.scope);
    const section = compiled.sources.find(source => source.sourceId === f.sources[0].sourceId)!.sectionKeys[0];
    expect(section).toBeTruthy();
    const search = f.snapshot(false, { forbiddenQueries: ["苹果"] }).find(tool => tool.name === "knowledge_search")!;
    const params = { scopeId: f.scope.id, query: "苹果", channel: "fts" };
    expect((await search.execute("section", { ...params, sectionKeys: [section] }, undefined, undefined, f.runtime())).isError).toBeFalsy();
    for (const sectionKeys of [[3], [""], ["不存在的章节"]]) {
      const invalid = await search.execute("invalid-section", { ...params, sectionKeys }, undefined, undefined, f.runtime());
      expect(invalid.details.errorCode).toBe("KNOWLEDGE_SCOPE_VIOLATION");
    }
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 4, searchCalls: 4 });
    expect(f.research.listActions(f.run.id).slice(1).map(action => action.errorCode))
      .toEqual(["KNOWLEDGE_SCOPE_VIOLATION", "KNOWLEDGE_SCOPE_VIOLATION", "KNOWLEDGE_SCOPE_VIOLATION"]);
  });

  it("同一查询调查不同来源允许执行，同一来源集合重新排序仍是重复", async () => {
    const f = await fixture(); const search = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    for (const source of f.sources) expect((await search.execute("source", { scopeId: f.scope.id, query: "苹果", channel: "fts",
      sourceIds: [source.sourceId] }, undefined, undefined, f.runtime())).isError).toBeFalsy();
    const both = f.sources.map(source => source.sourceId);
    expect((await search.execute("both", { scopeId: f.scope.id, query: "苹果", channel: "fts", sourceIds: both },
      undefined, undefined, f.runtime())).isError).toBeFalsy();
    expect((await search.execute("reordered", { scopeId: f.scope.id, query: "苹果", channel: "fts", sourceIds: [...both].reverse() },
      undefined, undefined, f.runtime())).details.errorCode).toBe("KNOWLEDGE_CONFLICT");
  });

  it("并发快照不能同时发起同范围等价查询，失败查询可以重新调查", async () => {
    const f = await fixture();
    let release!: () => void;
    const pending = new Promise<void>(resolve => { release = resolve; });
    const actualSearch = f.manager.searchService.search.bind(f.manager.searchService);
    const query = vi.spyOn(f.manager.searchService, "search").mockImplementationOnce(async request => {
      await pending;
      return actualSearch(request);
    });
    const first = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    const second = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    const running = first.execute("running", { scopeId: f.scope.id, query: "苹果", channel: "fts" }, undefined, undefined, f.runtime());
    try {
      await vi.waitFor(() => expect(query).toHaveBeenCalledOnce());
      expect((await second.execute("duplicate", { scopeId: f.scope.id, query: " 苹果 ", channel: "fts" },
        undefined, undefined, f.runtime())).details.errorCode).toBe("KNOWLEDGE_CONFLICT");
      expect(query).toHaveBeenCalledOnce();
    } finally { release(); }
    expect((await running).isError).toBeFalsy();
    query.mockRejectedValueOnce(new Error("临时检索失败"));
    expect((await first.execute("temporary-error", { scopeId: f.scope.id, query: "预算", channel: "fts" }, undefined, undefined, f.runtime())).isError).toBe(true);
    expect((await second.execute("retry", { scopeId: f.scope.id, query: "预算", channel: "fts" }, undefined, undefined, f.runtime())).isError).toBeFalsy();
    expect(query).toHaveBeenCalledTimes(3);
  });

  it("默认来源禁表不阻止显式不同来源，Root全范围历史也不阻止Worker独立调查", async () => {
    const f = await fixture();
    const root = f.snapshot(false, { forbiddenQueries: ["ＡＰＰＬＥ"] }).find(tool => tool.name === "knowledge_search")!;
    expect((await root.execute("forbidden", { scopeId: f.scope.id, query: "apple", channel: "fts" }, undefined, undefined, f.runtime())).isError).toBe(true);
    expect((await root.execute("narrow", { scopeId: f.scope.id, query: "apple", channel: "fts", sourceIds: [f.sources[0].sourceId] },
      undefined, undefined, f.runtime())).isError).toBeFalsy();
    const unrestricted = f.snapshot().find(tool => tool.name === "knowledge_search")!;
    expect((await unrestricted.execute("all", { scopeId: f.scope.id, query: "苹果", channel: "fts" }, undefined, undefined, f.runtime())).isError).toBeFalsy();
    const worker = f.snapshot(true, { forbiddenQueries: ["苹果"] }).find(tool => tool.name === "knowledge_search")!;
    expect((await worker.execute("worker", { scopeId: f.scope.id, query: "苹果", channel: "fts" }, undefined, undefined, f.runtime(true))).isError).toBeFalsy();
  });

  it("Worker阅读动作记录宿主分配需求，委派只保留分配需求对应计划且不落会话清单", async () => {
    const f = await fixture();
    const other = f.research.createNeed(f.run.id, { claim: "其它需求", kind: "fact", required: true,
      minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false });
    const planning = { searchPlan: [{ query: "shared", needIds: [f.need.id, other.id], purpose: "counterexample" as const },
      { query: "other", needIds: [other.id] }], forbiddenQueries: ["old query"] };
    const read = f.snapshot(true).find(tool => tool.name === "knowledge_read")!;
    expect((await read.execute("read", { scopeId: f.scope.id, sourceId: f.sources[0].sourceId }, undefined, undefined, f.runtime(true))).isError).toBeFalsy();
    expect(f.research.listActions(f.run.id)[0].requestSummary).toMatchObject({ needIds: [f.need.id], sourceIds: [f.sources[0].sourceId] });
    const delegate = f.snapshot(false, planning).find(tool => tool.name === "knowledge_delegate")!;
    expect((await delegate.execute("delegate", { runId: f.run.id, tasks: [{ label: "日期", needIds: [f.need.id], task: "查询日期" }] },
      undefined, undefined, f.runtime())).isError).toBeFalsy();
    expect(f.executeIsolated.mock.calls[0][1].research).toMatchObject({
      searchPlan: [{ query: "shared", needIds: [f.need.id], purpose: "counterexample" }], forbiddenQueries: ["old query"],
    });
    expect(JSON.stringify(f.manifests.getBySessionId(f.rootManifest.sessionId)?.provenance)).not.toContain("old query");
  });
});
