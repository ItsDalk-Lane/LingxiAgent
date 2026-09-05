import fs from "node:fs";
import { afterEach, describe, expect, it } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import type { KnowledgeResearchProgress } from "../shared/knowledge-research.ts";
import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish,
  type ResearchModelTurn } from "./helpers/knowledge-research-agent-fixture.ts";

const fixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
afterEach(async () => { for (const fixture of fixtures.splice(0)) await fixture.close(); });

async function setup(driver: (turn: ResearchModelTurn) => Promise<unknown>) {
  const f = await createResearchAgentFixture(driver); fixtures.push(f);
  const events: Array<{ event: KnowledgeResearchProgress; sessionPath: string; remainingSessions: number }> = [];
  const engine = Object.assign(Object.create(LingxiEngine.prototype) as LingxiEngine, {
    _knowledge: f.manager, _runtimeContext: { studioId: f.request.compiledScope.studioId },
    getSessionIdForPath: (sessionPath: string) => f.manifests.resolveByLocatorPath(sessionPath)?.sessionId ?? null,
    getSessionManifest: (sessionId: string) => f.manifests.getBySessionId(sessionId),
    executeIsolated: f.executeIsolated,
    emitEvent: (event: KnowledgeResearchProgress, sessionPath: string) => {
      events.push({ event, sessionPath, remainingSessions: f.sessionPaths.filter(file => fs.existsSync(file)).length });
    },
  });
  const input = { question: f.request.question,
    knowledgeRefs: { notebookIds: f.request.compiledScope.notebookIds, mode: "detailed" as const },
    sessionId: f.request.parentSessionId, sessionPath: f.request.parentSessionPath,
    agentId: f.request.agentId, turnId: f.request.turnId };
  return { ...f, engine, events, input };
}

describe("真实详细调查发布有限过程元数据", () => {
  it("计划、每轮、每个真实Worker及台账更新通过正式Engine送到主会话，全部清理后才完成", async () => {
    const f = await setup(async turn => {
      if (turn.role === "worker") {
        const need = f.research.getNeed(turn.runId, turn.options.research.allowedNeedIds[0]);
        await turn.call("knowledge_search", { scopeId: turn.scopeId, query: need.ordinal === 0 ? "日期" : "预算", channel: "fts" });
        await recordSourceEvidence(turn, need.id, f.sources[need.ordinal].sourceId, need.ordinal === 0 ? "九月十五日" : "三十二万元");
        return { stopReason: "stop", replyText: "模型隐藏推理绝不广播" };
      }
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const needs = (await turn.call("knowledge_research_update", { runId: turn.runId,
        createNeeds: [researchNeed("需求正文不广播A"), researchNeed("需求正文不广播B")] })).needs as Array<{ id: string }>;
      const delegated = await turn.call("knowledge_delegate", { runId: turn.runId,
        tasks: needs.map((need, index) => ({ label: `查证${index + 1}`, needIds: [need.id], task: "读取并登记；内部任务提示不广播" })) });
      expect(delegated.isError).toBeUndefined();
      expect((await requestFinish(turn)).accepted).toBe(true);
    });
    const result = await f.engine.buildDetailedKnowledgeResearchContext(f.input);
    const events = f.events.map(item => item.event);
    expect(new Set(events.map(event => event.type))).toEqual(new Set([
      "knowledge_research_started", "knowledge_research_plan_updated", "knowledge_research_round_started",
      "knowledge_research_worker_started", "knowledge_research_worker_completed", "knowledge_research_ledger_updated", "knowledge_research_completed",
    ]));
    expect(events[0]).toMatchObject({ type: "knowledge_research_started", needsTotal: 0, rounds: 0 });
    const planIndex = events.findIndex(event => event.type === "knowledge_research_plan_updated");
    expect(events[planIndex]).toMatchObject({ needsTotal: 2, needsSupported: 0 });
    const starts = events.filter(event => event.type === "knowledge_research_worker_started");
    const ends = events.filter(event => event.type === "knowledge_research_worker_completed");
    expect(starts).toHaveLength(2); expect(ends).toHaveLength(2);
    expect(new Set(starts.map(event => event.taskId)).size).toBe(2);
    for (const started of starts) {
      expect(events.indexOf(started)).toBeGreaterThan(planIndex);
      const ended = ends.find(event => event.taskId === started.taskId)!;
      expect(ended).toMatchObject({ label: started.label, status: "completed" });
      expect(events.indexOf(ended)).toBeGreaterThan(events.indexOf(started));
    }
    expect(events.some(event => event.type === "knowledge_research_ledger_updated" && event.phase === "investigating"
      && event.needsSupported > 0)).toBe(true);
    expect(events.at(-2)).toMatchObject({ type: "knowledge_research_ledger_updated", phase: "reviewing", needsSupported: 2 });
    expect(events.at(-1)).toMatchObject({ type: "knowledge_research_completed", status: "completed", rounds: 1,
      searchCalls: 2, readCalls: 2, delegatedAgents: 2, needsTotal: 2, needsSupported: 2, unresolvedNeedIds: [], stopReason: "complete" });
    expect(f.events.at(-1)!.remainingSessions).toBe(0);
    expect(f.events.every(item => item.sessionPath === f.input.sessionPath && item.event.runId === result.stats.research!.runId
      && item.event.scopeId === result.stats.scopeId)).toBe(true);
    expect(JSON.stringify(events)).not.toMatch(/模型隐藏推理|需求正文|内部任务提示|九月十五日|三十二万元|SELECT|receiptId/);
  });

  it("取消Worker也等待隔离资源清理后关闭任务和调查，不发送成功终态", async () => {
    const controller = new AbortController();
    const f = await setup(async turn => {
      if (turn.role === "worker") { controller.abort(); return { stopReason: "aborted" }; }
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const need = (await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("日期")] })).needs[0];
      await turn.call("knowledge_delegate", { runId: turn.runId, tasks: [{ label: "查日期", needIds: [need.id], task: "核对日期" }] });
    });
    await expect(f.engine.buildDetailedKnowledgeResearchContext({ ...f.input, signal: controller.signal })).rejects.toMatchObject({ name: "AbortError" });
    const events = f.events.map(item => item.event);
    expect(events.filter(event => event.type === "knowledge_research_worker_started")).toHaveLength(1);
    expect(events.filter(event => event.type === "knowledge_research_worker_completed")).toEqual([
      expect.objectContaining({ status: "cancelled" }),
    ]);
    expect(events.at(-1)).toMatchObject({ type: "knowledge_research_completed", status: "cancelled" });
    expect(f.events.at(-1)!.remainingSessions).toBe(0);
    expect(events.filter(event => event.type === "knowledge_research_completed")).toHaveLength(1);
  });

  it("取消期间冻结范围关闭后仍发送持久化的取消终态，不能将渲染拒绝误报为调查失败", async () => {
    const controller = new AbortController();
    const f = await setup(async () => {
      f.manager.closeTurnScope({ scopeId: f.request.compiledScope.scopeId });
      controller.abort();
      return { stopReason: "aborted" };
    });
    await expect(f.engine.buildDetailedKnowledgeResearchContext({ ...f.input, signal: controller.signal }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    const events = f.events.map(item => item.event);
    const run = f.research.requireRun(events[0].runId);
    expect(run).toMatchObject({ status: "cancelled", stopReason: "cancelled" });
    expect(events.filter(event => event.type === "knowledge_research_completed")).toEqual([
      expect.objectContaining({ status: "cancelled", stopReason: "cancelled" }),
    ]);
    expect(f.events.at(-1)!.remainingSessions).toBe(0);
    expect(f.sessionPaths.every(file => !fs.existsSync(file))).toBe(true);
  });
});
