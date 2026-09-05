import { afterEach, describe, expect, it } from "vitest";
import { KnowledgeResearchOrchestrator } from "../lib/knowledge/research/knowledge-research-orchestrator.ts";
import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish } from "./helpers/knowledge-research-agent-fixture.ts";

const fixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
afterEach(async () => { for (const fixture of fixtures.splice(0)) await fixture.close(); });

describe("多维调查实际委派", () => {
  it("同一来源的两次查询不能冒充另一个需求的独立调查", async () => {
    let round = 0;
    const f = await createResearchAgentFixture(async turn => {
      if (++round === 1) {
        await turn.call("knowledge_outline", { scopeId: turn.scopeId });
        const needs = (await turn.call("knowledge_research_update", { runId: turn.runId,
          createNeeds: [researchNeed("日期"), researchNeed("预算")] })).needs;
        for (const query of ["交付", "日期"]) {
          const found = await turn.call("knowledge_search", { scopeId: turn.scopeId, query,
            sourceIds: [f.sources[0].sourceId], channel: "fts" });
          expect(found.hits.length).toBeGreaterThan(0);
        }
        await recordSourceEvidence(turn, needs[0].id, f.sources[0].sourceId, "九月十五日");
        await recordSourceEvidence(turn, needs[1].id, f.sources[1].sourceId, "三十二万元");
      }
      await requestFinish(turn);
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "partial", stopReason: "agent_protocol_failure", roundsCompleted: 2 });
    expect(result.packet.canonicalEvidenceSpans).toHaveLength(2);
  });

  it("两名Worker按分配需求取原文，显式Agent与Root默认Agent均沿真实两层父链入账", async () => {
    const f = await createResearchAgentFixture(async turn => {
      if (turn.role === "worker") {
        const need = f.research.getNeed(turn.runId, turn.options.research.allowedNeedIds[0]);
        await turn.call("knowledge_search", { scopeId: turn.scopeId, query: need.ordinal === 0 ? "日期" : "预算", channel: "fts" });
        await recordSourceEvidence(turn, need.id, f.sources[need.ordinal].sourceId, need.ordinal === 0 ? "九月十五日" : "三十二万元");
        return { stopReason: "stop", replyText: "Worker私有推理不应出现" };
      }
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const needs = (await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("日期"), researchNeed("预算")] })).needs;
      const delegated = await turn.call("knowledge_delegate", { runId: turn.runId,
        tasks: needs.map((need: { id: string }, index: number) => ({ label: `调查${index + 1}`, needIds: [need.id], task: "找到原文并登记",
          ...(index === 1 ? { agentId: "agent-b" } : {}) })) });
      expect(JSON.stringify(delegated)).not.toContain("Worker私有");
      await requestFinish(turn);
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "completed", delegatedAgents: 2, toolCallsUsed: 10, searchCalls: 2, readCalls: 2 });
    expect(f.calls.filter(call => call.role === "worker").map(call => call.options.agentId).sort()).toEqual(["agent-a", "agent-b"]);
    expect(result.packet.needs.every(need => need.status === "supported")).toBe(true);
    expect(result.block).not.toContain("Worker私有");
  });
});
