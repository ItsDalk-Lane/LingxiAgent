import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish, type ResearchModelTurn } from "./helpers/knowledge-research-agent-fixture.ts";

const fixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
afterEach(async () => { for (const fixture of fixtures.splice(0)) await fixture.close(); });

async function setup(driver: (turn: ResearchModelTurn) => Promise<unknown>) {
  const fixture = await createResearchAgentFixture(driver);
  fixtures.push(fixture);
  const engine = Object.assign(Object.create(LingxiEngine.prototype) as LingxiEngine, {
    _knowledge: fixture.manager, _runtimeContext: { studioId: fixture.request.compiledScope.studioId },
    getSessionIdForPath: (sessionPath: string) => fixture.manifests.resolveByLocatorPath(sessionPath)?.sessionId ?? null,
    getSessionManifest: (sessionId: string) => fixture.manifests.getBySessionId(sessionId),
    executeIsolated: fixture.executeIsolated, emitEvent: vi.fn(),
  });
  const input = { question: fixture.request.question,
    knowledgeRefs: { notebookIds: fixture.request.compiledScope.notebookIds, mode: "detailed" as const },
    sessionId: fixture.request.parentSessionId, sessionPath: fixture.request.parentSessionPath,
    agentId: fixture.request.agentId, turnId: fixture.request.turnId };
  const existingRun = () => fixture.research.createRun({ turnScopeId: fixture.request.compiledScope.scopeId,
    turnId: input.turnId, parentSessionPath: input.sessionPath, question: input.question });
  return { ...fixture, engine, input, existingRun };
}

describe("详细研究入口恢复冻结范围和预算", () => {
  it("同一轮已经超过绝对时限时复用原研究，不再次调用模型或重新发预算", async () => {
    const fixture = await setup(async () => { throw new Error("已到截止时间，模型不应再次运行"); });
    const old = fixture.existingRun();
    const createdAt = new Date(Date.now() - old.budget.maxWallClockMs - 1000).toISOString();
    fixture.manager.store.db.prepare("UPDATE knowledge_research_runs SET tool_calls_used = 31, created_at = ? WHERE id = ?")
      .run(createdAt, old.id);
    const result = await fixture.engine.buildDetailedKnowledgeResearchContext(fixture.input);
    expect(result.stats.scopeId).toBe(old.turnScopeId);
    expect(result.stats.research).toMatchObject({ runId: old.id, status: "partial", stopReason: "wall_clock_exhausted", toolCalls: 31 });
    expect(fixture.research.requireRun(old.id).createdAt).toBe(createdAt);
    expect(fixture.manager.getTurnScope({ scopeId: old.turnScopeId })?.status).toBe("active");
    expect(fixture.calls).toHaveLength(0);
    expect(fixture.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_runs").get().count).toBe(1);
  });

  it("旧研究已经用掉31次时只剩一次调用，计数和开始时间不因恢复改变", async () => {
    const fixture = await setup(async turn => {
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      return { stopReason: "stop" };
    });
    const old = fixture.existingRun();
    const createdAt = new Date(Date.now() - 60_000).toISOString();
    fixture.manager.store.db.prepare("UPDATE knowledge_research_runs SET tool_calls_used = 31, created_at = ? WHERE id = ?")
      .run(createdAt, old.id);
    const result = await fixture.engine.buildDetailedKnowledgeResearchContext(fixture.input);
    expect(result.stats.scopeId).toBe(old.turnScopeId);
    expect(result.stats.research).toMatchObject({ runId: old.id, status: "partial", stopReason: "tool_budget_exhausted", toolCalls: 32 });
    expect(fixture.research.requireRun(old.id).createdAt).toBe(createdAt);
    expect(fixture.research.listActions(old.id)).toHaveLength(1);
    expect(fixture.calls).toHaveLength(1);
    expect(fixture.manager.getTurnScope({ scopeId: old.turnScopeId })?.status).toBe("active");
  });

  it("同一轮并发请求被拒绝且不关闭首个调查的冻结范围", async () => {
    let release!: () => void;
    const waiting = new Promise<void>(resolve => { release = resolve; });
    let modelCalls = 0;
    const fixture = await setup(async turn => {
      if (++modelCalls === 1) await waiting;
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      await requestFinish(turn);
    });
    const first = fixture.engine.buildDetailedKnowledgeResearchContext(fixture.input);
    // 无论第二个入口是否正确拒绝，都等首个调查真正结束后才关闭测试数据库。
    const settled = first.then(result => ({ result }), error => ({ error }));
    let firstScopeId = "", firstRunId = "";
    try {
      await vi.waitFor(() => expect(fixture.calls).toHaveLength(1));
      firstScopeId = fixture.calls[0].scopeId; firstRunId = fixture.calls[0].runId;
      await expect(fixture.engine.buildDetailedKnowledgeResearchContext(fixture.input))
        .rejects.toMatchObject({ code: "KNOWLEDGE_CONFLICT" });
      expect(fixture.manager.getTurnScope({ scopeId: firstScopeId })?.status).toBe("active");
      expect(fixture.calls).toHaveLength(1);
      expect(fixture.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_runs").get().count).toBe(1);
    } finally { release(); await settled; }
    const completed = await settled;
    expect("error" in completed).toBe(false);
    if ("result" in completed) expect(completed.result.stats.research).toMatchObject({ runId: firstRunId, status: "partial", stopReason: "no_progress" });
    expect(fixture.manager.getTurnScope({ scopeId: firstScopeId })?.status).toBe("active");
  });

  it("同一轮已完成后再次请求复用原研究和证据，不启动模型或重发预算", async () => {
    let modelCalls = 0;
    const fixture = await setup(async turn => {
      if (++modelCalls > 1) throw new Error("已完成研究不应再次启动模型");
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const update = await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("确认交付日期")] });
      await recordSourceEvidence(turn, update.needs[0].id, fixture.sources[0].sourceId, "九月十五日");
      expect((await requestFinish(turn)).accepted).toBe(true);
    });
    const first = await fixture.engine.buildDetailedKnowledgeResearchContext(fixture.input);
    expect(first.stats.research).toMatchObject({ status: "completed", toolCalls: 5, rounds: 1 });
    expect(first.evidence.entries).toHaveLength(1);
    const second = await fixture.engine.buildDetailedKnowledgeResearchContext(fixture.input);
    expect(second.stats.scopeId).toBe(first.stats.scopeId);
    expect(second.stats.research).toMatchObject({ runId: first.stats.research!.runId, status: "completed", toolCalls: 5, rounds: 1 });
    expect(second.evidence).toEqual(first.evidence);
    expect(second.block).toBe(first.block);
    expect(modelCalls).toBe(1);
    expect(fixture.research.listActions(first.stats.research!.runId)).toHaveLength(5);
    expect(fixture.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_runs").get().count).toBe(1);
  });
});
