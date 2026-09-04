import { afterEach, describe, expect, it } from "vitest";
import { KnowledgeResearchOrchestrator } from "../lib/knowledge/research/knowledge-research-orchestrator.ts";
import { createResearchAgentFixture, researchNeed, requestFinish } from "./helpers/knowledge-research-agent-fixture.ts";

const fixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
afterEach(async () => { for (const fixture of fixtures.splice(0)) await fixture.close(); });

describe("无新增有效证据的停止", () => {
  it("完成协议但连续两轮没有原文入账时停止，搜索摘要不能冒充有效证据", async () => {
    let round = 0;
    const f = await createResearchAgentFixture(async turn => {
      if (++round === 1) {
        await turn.call("knowledge_outline", { scopeId: turn.scopeId });
        await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("未查明的预算")] });
        const found = await turn.call("knowledge_search", { scopeId: turn.scopeId, query: "预算", channel: "fts" });
        expect(found.hits.length).toBeGreaterThan(0);
      }
      expect((await requestFinish(turn)).accepted).toBe(false);
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "partial", stopReason: "no_progress", roundsCompleted: 2 });
    expect(result.packet.canonicalEvidenceSpans).toEqual([]);
    expect(result.block).not.toContain("三十二万元");
    expect(f.calls[1].options.research.forbiddenQueries).toContain("预算");
    expect(f.research.listRounds(result.run.id).map(round => round.newEvidenceCount)).toEqual([0, 0]);
  });
});
