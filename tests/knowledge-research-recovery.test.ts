import fs from "node:fs";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnowledgeResearchOrchestrator } from "../lib/knowledge/research/knowledge-research-orchestrator.ts";
import { ResearchStore } from "../lib/knowledge/research/research-store.ts";
import { ResearchToolBudget } from "../lib/knowledge/research/research-tool-budget.ts";
import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish } from "./helpers/knowledge-research-agent-fixture.ts";

const fixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
afterEach(async () => { vi.restoreAllMocks(); for (const fixture of fixtures.splice(0)) await fixture.close(); });

function persistedRun(f: Awaited<ReturnType<typeof createResearchAgentFixture>>, elapsedMs: number) {
  const createdAt = new Date(Date.now() - elapsedMs).toISOString();
  const store = new ResearchStore(f.manager.store, { now: () => createdAt });
  const run = store.createRun({ turnScopeId: f.request.compiledScope.scopeId, turnId: f.request.turnId,
    parentSessionPath: f.request.parentSessionPath, question: f.request.question,
    completenessPolicy: f.request.policy.completenessPolicy });
  const round = store.beginRound(run.id, { focus: [] });
  return { run, round, createdAt };
}

/** 已写动作和计数后进程退出的持久化形态；没有留下任何内存中的控制器或可恢复模型会话。 */
function interruptedAction(f: Awaited<ReturnType<typeof createResearchAgentFixture>>, runId: string, roundId: string, sessionId: string) {
  const actions = f.research.listActions(runId);
  const action = f.research.insertAction({ id: f.research.newId("interrupted"), runId, roundId,
    ordinal: actions.length, actorSessionId: sessionId, actorAgentId: "agent-a", actionType: "knowledge_search",
    requestSummary: { query: "中断的查询" }, responseSummary: null, status: "running",
    startedAt: f.research.now(), completedAt: null, errorCode: null });
  f.manager.store.db.prepare(`UPDATE knowledge_research_runs SET tool_calls_used = tool_calls_used + 1,
    search_calls = search_calls + 1 WHERE id = ?`).run(runId);
  return action;
}

describe("研究恢复与收尾的真实持久化边界", () => {
  it("复用已有研究和当前轮次，不重发时间或调用预算，也不读取旧会话推理", async () => {
    const f = await createResearchAgentFixture(async turn => {
      expect(turn.prompt).not.toContain("禁止读取的崩溃前隐藏推理");
      expect(turn.options).not.toHaveProperty("resumeSessionPath");
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const need = (await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("交付日期")] })).needs[0];
      await recordSourceEvidence(turn, need.id, f.sources[0].sourceId, "九月十五日");
      await requestFinish(turn);
    }); fixtures.push(f);
    const saved = persistedRun(f, 170_000);
    const oldPath = path.join(path.dirname(f.request.parentSessionPath), "crashed-root.jsonl");
    fs.writeFileSync(oldPath, "禁止读取的崩溃前隐藏推理");
    const oldManifest = f.manifests.createForPath({ sessionPath: oldPath, ownerAgentId: "agent-a", domain: "subagent",
      kind: "knowledge_research_root", provenance: { parentSessionId: f.request.parentSessionId,
        studioId: f.request.compiledScope.studioId, researchContext: { runId: saved.run.id, scopeId: saved.run.turnScopeId, role: "root" } } });
    const budget = new ResearchToolBudget(f.research);
    await budget.execute({ context: { runId: saved.run.id, scopeId: saved.run.turnScopeId,
      actorSessionId: oldManifest.sessionId, actorAgentId: "agent-a", role: "root" }, toolName: "knowledge_outline", requestSummary: {} },
      () => ({ value: true, summary: { count: 3 } }));
    const interrupted = interruptedAction(f, saved.run.id, saved.round.id, oldManifest.sessionId);
    const originalRead = fs.readFileSync.bind(fs);
    const oldReadAttempts: string[] = [];
    vi.spyOn(fs, "readFileSync").mockImplementation((file, options) => {
      if (String(file) === oldPath) { oldReadAttempts.push(oldPath); throw new Error("旧会话不得重读"); }
      return originalRead(file, options);
    });
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ id: saved.run.id, createdAt: saved.createdAt, roundsCompleted: 1,
      status: "completed", degradedReason: "research_round_restarted" });
    expect(result.run.toolCallsUsed).toBe(f.research.listActions(saved.run.id).length);
    expect(result.run.toolCallsUsed).toBeGreaterThan(2);
    expect(budget.deadlineMs(saved.run.id)).toBe(Date.parse(saved.createdAt) + 180_000);
    expect(f.research.listRounds(saved.run.id)).toHaveLength(1);
    expect(f.research.listRounds(saved.run.id)[0]).toMatchObject({ id: saved.round.id, status: "completed" });
    expect(f.research.listActions(saved.run.id).find(action => action.id === interrupted.id))
      .toMatchObject({ status: "cancelled", errorCode: "RESEARCH_HOST_RESTARTED" });
    expect(f.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_runs").get().count).toBe(1);
    expect(oldReadAttempts).toEqual([]);
    expect(f.sessionPaths).not.toContain(oldPath);
    expect(f.sessionPaths.every(file => !fs.existsSync(file))).toBe(true);
  });

  it("恢复时原始deadline已过，先收尾遗留running轮再输出partial，不启动新模型", async () => {
    const f = await createResearchAgentFixture(async () => { throw new Error("过期研究不能再次启动模型"); }); fixtures.push(f);
    const saved = persistedRun(f, 181_000), action = interruptedAction(f, saved.run.id, saved.round.id, "crashed-root");
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ id: saved.run.id, createdAt: saved.createdAt, status: "partial",
      stopReason: "wall_clock_exhausted", roundsCompleted: 1, toolCallsUsed: 1 });
    expect(f.calls).toEqual([]);
    expect(f.research.listRounds(saved.run.id)).toHaveLength(1);
    expect(f.research.listRounds(saved.run.id)[0].status).not.toBe("running");
    expect(f.research.listActions(saved.run.id).find(item => item.id === action.id)?.status).toBe("cancelled");
    expect(f.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_runs").get().count).toBe(1);
  });

  it("已完成调查且进入synthesizing后崩溃，只复用原账本完成材料，不启动新模型", async () => {
    let preparing = true;
    const f = await createResearchAgentFixture(async turn => {
      if (!preparing) throw new Error("调查已经完成，恢复时不得再次启动模型");
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const need = (await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("交付日期")] })).needs[0];
      await recordSourceEvidence(turn, need.id, f.sources[0].sourceId, "九月十五日");
      expect((await requestFinish(turn)).accepted).toBe(true);
    }); fixtures.push(f);
    const saved = persistedRun(f, 0);
    await f.executeIsolated("真实调查并持久化证据", { surface: "knowledge_research_root", agentId: f.request.agentId,
      parentSessionId: f.request.parentSessionId, parentSessionPath: f.request.parentSessionPath,
      research: { runId: saved.run.id, scopeId: saved.run.turnScopeId, studioId: f.request.compiledScope.studioId },
      signal: new AbortController().signal });
    f.research.finishRound(saved.run.id, saved.round.id, { status: "completed", newEvidenceCount: 1, errorCode: null });
    f.research.beginSynthesis(saved.run.id);
    f.research.setRunState(saved.run.id, { status: "synthesizing", stopReason: "complete" });
    const before = f.research.requireRun(saved.run.id), actionCount = f.research.listActions(saved.run.id).length;
    preparing = false;
    const executeIsolated = vi.fn(f.executeIsolated);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ id: saved.run.id, createdAt: saved.createdAt, status: "completed", stopReason: "complete",
      roundsCompleted: 1, toolCallsUsed: before.toolCallsUsed, searchCalls: before.searchCalls, readCalls: before.readCalls });
    expect(result.packet.canonicalEvidenceSpans.map(span => span.text)).toEqual(["九月十五日"]);
    expect(executeIsolated).not.toHaveBeenCalled();
    expect(f.research.listActions(saved.run.id)).toHaveLength(actionCount);
    expect(f.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_runs").get().count).toBe(1);
  });

  it("最终重新读证据发现真实正文损坏时保留失败终态，不悬在synthesizing", async () => {
    let runId = "";
    const f = await createResearchAgentFixture(async turn => {
      runId = turn.runId;
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const need = (await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("交付日期")] })).needs[0];
      await recordSourceEvidence(turn, need.id, f.sources[0].sourceId, "九月十五日");
      expect((await requestFinish(turn)).accepted).toBe(true);
      const evidence = f.research.listEvidence(turn.runId)[0];
      f.manager.store.db.prepare("UPDATE knowledge_blocks SET text = text || '损坏' WHERE id = ?").run(evidence.blockId);
    }); fixtures.push(f);
    await expect(new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request))
      .rejects.toMatchObject({ code: "KNOWLEDGE_STORAGE_INVALID" });
    expect(f.research.requireRun(runId).status).toBe("failed");
    expect(f.research.requireRun(runId).completedAt).not.toBeNull();
    expect(f.sessionPaths.every(file => !fs.existsSync(file))).toBe(true);
  });

  it("连续两轮某工具失败但原文阅读和有效证据持续成功时，不误判关键链路整体不可用", async () => {
    let round = 0, needId = "";
    const f = await createResearchAgentFixture(async turn => {
      if (++round === 1) {
        await turn.call("knowledge_outline", { scopeId: turn.scopeId });
        needId = (await turn.call("knowledge_research_update", { runId: turn.runId,
          createNeeds: [researchNeed("各份项目资料的依据", { minIndependentSources: 3 })] })).needs[0].id;
      }
      expect(await turn.call("knowledge_grep", { scopeId: turn.scopeId, pattern: "[", regexp: true })).toMatchObject({ isError: true });
      await recordSourceEvidence(turn, needId, f.sources[round - 1].sourceId, f.sources[round - 1].text);
      await requestFinish(turn);
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "completed", stopReason: "complete", roundsCompleted: 3 });
    expect(f.research.listRounds(result.run.id).map(item => item.newEvidenceCount)).toEqual([1, 1, 1]);
    expect(result.packet.canonicalEvidenceSpans).toHaveLength(3);
  });
});
