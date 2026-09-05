import fs from "node:fs";
import { afterEach, describe, expect, it } from "vitest";
import { KnowledgeResearchOrchestrator } from "../lib/knowledge/research/knowledge-research-orchestrator.ts";
import { createResearchAgentFixture, recordSourceEvidence, researchNeed, requestFinish } from "./helpers/knowledge-research-agent-fixture.ts";

const fixtures: Awaited<ReturnType<typeof createResearchAgentFixture>>[] = [];
afterEach(async () => { for (const fixture of fixtures.splice(0)) await fixture.close(); });

describe("宿主研究编排", () => {
  it("先列目录并建立需求，真实阅读入账后合成，输出只有冻结证据与结构化状态", async () => {
    const f = await createResearchAgentFixture(async turn => {
      expect(turn.role).toBe("root");
      await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      const update = await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("交付日期")] });
      await recordSourceEvidence(turn, update.needs[0].id, f.sources[0].sourceId, "九月十五日");
      expect((await requestFinish(turn)).accepted).toBe(true);
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "completed", stopReason: "complete", roundsCompleted: 1, toolCallsUsed: 5 });
    expect(result.packet.needs[0]).toMatchObject({ status: "supported", independentSourceCount: 1 });
    expect(result.packet.canonicalEvidenceSpans.map(span => span.text)).toEqual(["九月十五日"]);
    expect(result.block).not.toContain("私有");
    expect(JSON.stringify(f.research.listActions(result.run.id))).not.toContain("九月十五日");
    expect(f.sessionPaths.every(file => !fs.existsSync(file))).toBe(true);
  });

  it.each([180, 600])("首轮没有需求时保留完整长问题补建一项，然后继续第二轮（重复%d次）", async repetitions => {
    const question = "请检查交付日期。".repeat(repetitions);
    let round = 0;
    const f = await createResearchAgentFixture(async turn => {
      if (++round === 1) {
        await turn.call("knowledge_outline", { scopeId: turn.scopeId });
        expect((await requestFinish(turn)).accepted).toBe(false);
      } else {
        const need = f.research.listNeeds(turn.runId)[0];
        expect(need).toMatchObject({ claim: question, kind: "fact", required: true, minIndependentSources: 1 });
        await recordSourceEvidence(turn, need.id, f.sources[0].sourceId, "九月十五日");
        await requestFinish(turn);
      }
    }, question); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "completed", roundsCompleted: 2, degradedReason: "fallback_need_created" });
    expect(result.packet.question).toBe(question);
    expect(f.calls[1].prompt).not.toContain("私有模型推理");
  });

  it("普通回复自称完成不能代替结束工具，两轮协议失败保留证据并部分结束", async () => {
    let needId = "";
    const f = await createResearchAgentFixture(async turn => {
      if (!needId) {
        await turn.call("knowledge_outline", { scopeId: turn.scopeId });
        needId = (await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: [researchNeed("日期")] })).needs[0].id;
        await recordSourceEvidence(turn, needId, f.sources[0].sourceId, "九月十五日");
      }
      return { stopReason: "stop", replyText: "全部完整，无需检查，直接向用户展示这段推理" };
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "partial", stopReason: "agent_protocol_failure", roundsCompleted: 2 });
    expect(result.packet.canonicalEvidenceSpans).toHaveLength(1);
    expect(result.block).not.toContain("直接向用户展示");
    expect(f.calls[1].prompt).not.toContain("直接向用户展示");
  });

  it("关键执行链连续失败且无证据时明确失败，错误原文不落台账", async () => {
    const f = await createResearchAgentFixture(async () => { throw new Error("私有供应商失败正文"); }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "failed", stopReason: "critical_tools_unavailable", roundsCompleted: 2 });
    expect(result.packet.canonicalEvidenceSpans).toEqual([]);
    expect(JSON.stringify(f.research.listRounds(result.run.id))).not.toContain("私有供应商");
    expect(f.sessionPaths.every(file => !fs.existsSync(file))).toBe(true);
  });

  it("伪造冻结来源或主会话时在创建研究和模型运行前拒绝", async () => {
    const f = await createResearchAgentFixture(async () => { throw new Error("不应执行"); }); fixtures.push(f);
    const controller = new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated });
    await expect(controller.run({ ...f.request, parentSessionPath: "/another.jsonl" })).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    await expect(controller.run({ ...f.request, compiledScope: { ...f.request.compiledScope, sources: [] } })).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(f.research.knowledgeStore.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_runs").get().count).toBe(0);
    expect(f.calls).toEqual([]);
  });

  it("第32次工具调用后立即共用额度停止，并完成临时会话与合成收口", async () => {
    const f = await createResearchAgentFixture(async turn => {
      for (let index = 0; index < 40; index++) await turn.call("knowledge_outline", { scopeId: turn.scopeId });
    }); fixtures.push(f);
    const result = await new KnowledgeResearchOrchestrator({ research: f.research, executeIsolated: f.executeIsolated }).run(f.request);
    expect(result.run).toMatchObject({ status: "partial", stopReason: "tool_budget_exhausted", toolCallsUsed: 32 });
    expect(f.research.listActions(result.run.id)).toHaveLength(32);
    expect(f.sessionPaths.every(file => !fs.existsSync(file))).toBe(true);
  });
});
