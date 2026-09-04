import { afterEach, describe, expect, it } from "vitest";
import { KnowledgeResearchOrchestrator } from "../lib/knowledge/research/knowledge-research-orchestrator.ts";
import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish } from "./helpers/knowledge-research-agent-fixture.ts";

const fixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
afterEach(async () => { for (const fixture of fixtures.splice(0)) await fixture.close(); });

describe("研究按缺口继续", () => {
  it("多维问题先解决一项，第二轮仅委派未解决项，真实Worker共用账本", async () => {
    let roots = 0;
    const f = await createResearchAgentFixture(async turn => {
      if (turn.role === "worker") {
        const needId = turn.options.research.allowedNeedIds[0];
        const need = f.research.getNeed(turn.runId, needId);
        await recordSourceEvidence(turn, needId, f.sources[need.ordinal].sourceId, need.ordinal === 0 ? "九月十五日" : "三十二万元");
        return;
      }
      if (++roots === 1) {
        await turn.call("knowledge_outline", { scopeId: turn.scopeId });
        await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("日期"), researchNeed("预算")] });
      }
      const needs = f.research.listNeeds(turn.runId);
      await turn.call("knowledge_delegate", { runId: turn.runId, tasks: [{ label: "定向核对", needIds: [needs[roots - 1].id], task: "只核查本次分配需求" }] });
      expect((await requestFinish(turn)).accepted).toBe(roots === 2);
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "completed", roundsCompleted: 2, delegatedAgents: 2 });
    const rounds = f.research.listRounds(result.run.id);
    expect(rounds.map(round => round.newEvidenceCount)).toEqual([1, 1]);
    expect(rounds[1].focus).toEqual([result.packet.needs[1].id]);
    expect(f.calls.filter(call => call.role === "worker")).toHaveLength(2);
    expect(f.calls.filter(call => call.role === "root")[1].prompt).not.toContain("私有模型推理");
  });

  it("矛盾证据不会被普通完成声明消除，后续轮明确聚焦冲突并保留双方原文", async () => {
    let roots = 0;
    const f = await createResearchAgentFixture(async turn => {
      if (++roots === 1) {
        await turn.call("knowledge_outline", { scopeId: turn.scopeId });
        const need = (await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("交付日期")] })).needs[0];
        await recordSourceEvidence(turn, need.id, f.sources[0].sourceId, "九月十五日");
        await recordSourceEvidence(turn, need.id, f.sources[2].sourceId, "九月二十日", "contradicts");
      }
      expect((await requestFinish(turn)).accepted).toBe(false);
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "partial", stopReason: "no_progress", roundsCompleted: 3 });
    expect(result.packet.needs[0]).toMatchObject({ status: "conflicted" });
    expect(result.packet.needs[0].contradictingEvidenceIds).toHaveLength(1);
    expect(result.packet.canonicalEvidenceSpans.map(span => span.text).sort()).toEqual(["九月二十日", "九月十五日"].sort());
    expect(f.calls[1].options.research.searchPlan[0].query).toContain("矛盾");
  });

  it("每轮虽有新证据仍不能越过四轮共享上限，来源要求不足保留部分状态", async () => {
    let round = 0, needId = "";
    const f = await createResearchAgentFixture(async turn => {
      if (++round === 1) {
        await turn.call("knowledge_outline", { scopeId: turn.scopeId });
        needId = (await turn.call("knowledge_research_update", { runId: turn.runId,
          createNeeds: [researchNeed("汇总项目依据", { minIndependentSources: 4 })] })).needs[0].id;
      }
      const index = (round - 1) % 3;
      await recordSourceEvidence(turn, needId, f.sources[index].sourceId, round === 4 ? "苹果项目交付日期" : f.sources[index].text);
      await requestFinish(turn);
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "partial", stopReason: "round_budget_exhausted", roundsCompleted: 4 });
    expect(f.research.listRounds(result.run.id).map(round => round.newEvidenceCount)).toEqual([1, 1, 1, 1]);
    expect(result.packet.needs[0].independentSourceCount).toBe(3);
  });
});
