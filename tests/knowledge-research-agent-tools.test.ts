import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Agent } from "../core/agent.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { ResearchStore } from "../lib/knowledge/research/research-store.ts";
import { resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";
import { getKnowledgeResearchToolNames } from "../shared/tool-categories.ts";
import type { KnowledgeResearchActorContext } from "../lib/knowledge/research/research-tool-budget.ts";

const cleanup: Array<() => Promise<void>> = [];
afterEach(async () => { for (const close of cleanup.splice(0)) await close(); });
async function fixture() {
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
  const snapshot = (worker = false) => agent.getToolsSnapshot({ surface: worker ? "knowledge_research_worker" : "knowledge_research_root",
    research: { runId: run.id, scopeId: scope.id, studioId, actorContext: context(worker), sessionPath: (worker ? workerManifest : rootManifest).currentLocator.path } });
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
});
