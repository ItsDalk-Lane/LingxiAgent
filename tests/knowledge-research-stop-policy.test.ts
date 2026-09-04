import { afterEach, describe, expect, it } from "vitest";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { evaluateResearchStopPolicy, type KnowledgeResearchStopPolicyInput } from "../lib/knowledge/research/research-stop-policy.ts";
import type { KnowledgeEvidenceNeedRecord, KnowledgeResearchRun } from "../lib/knowledge/types.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const fixtures: ReturnType<typeof createKnowledgeResearchFixture>[] = [];
afterEach(() => { for (const fixture of fixtures.splice(0)) fixture.close(); });

function setup(options: { completenessPolicy?: KnowledgeResearchRun["completenessPolicy"];
  budget?: Partial<KnowledgeResearchRun["budget"]> } = {}) {
  const fixture = createKnowledgeResearchFixture();
  fixtures.push(fixture);
  const run = fixture.research.createRun({ turnScopeId: fixture.scope.id, turnId: fixture.scope.turnId,
    parentSessionPath: fixture.scope.sessionPath, question: "核实项目事实", ...options,
    budget: { ...fixture.run.budget, ...options.budget } });
  const provenNeedIds = new Set<string>();
  const ledger = new EvidenceLedger(fixture.research, {
    isCompletenessSatisfied: (runId, needId) => runId === run.id && provenNeedIds.has(needId),
  });
  const need = (overrides: Partial<Pick<KnowledgeEvidenceNeedRecord, "required" | "minIndependentSources"
    | "requireCounterEvidence" | "requireAllRelevantUnits">> = {}) => fixture.research.createNeed(run.id, {
    claim: "核实交付时间", kind: "fact", required: true, minIndependentSources: 1,
    requireCounterEvidence: false, requireAllRelevantUnits: false, ...overrides,
  });
  const link = (needId: string, sourceIndex = 0, relation: "supports" | "contradicts" = "supports") => {
    const source = fixture.sources[sourceIndex];
    const receipt = fixture.receipts.issue({ runId: run.id, actorSessionId: null, ...source,
      startOffset: 0, endOffset: source.text.length, channel: "knowledge_read" });
    return ledger.linkEvidence({ runId: run.id, needId, receiptId: receipt.id,
      quote: source.text, relation, rationale: "已读取并核对原文" });
  };
  const decide = (input: Partial<Omit<KnowledgeResearchStopPolicyInput, "run" | "needs" | "recentRoundEvidenceCounts">> = {}) =>
    evaluateResearchStopPolicy({ run: fixture.research.requireRun(run.id), needs: ledger.recompute(run.id),
      recentRoundEvidenceCounts: fixture.research.listRounds(run.id).filter(round => round.status !== "running").map(round => round.newEvidenceCount),
      elapsedMs: 0, completenessSatisfied: false, ...input });
  const finishRound = (newEvidenceCount: number) => {
    const round = fixture.research.beginRound(run.id, { focus: fixture.research.listNeeds(run.id).map(item => item.id) });
    fixture.research.finishRound(run.id, round.id, { status: "completed", newEvidenceCount, errorCode: null });
  };
  return { ...fixture, run, ledger, provenNeedIds, need, link, decide, finishRound };
}

describe("研究停止门禁使用宿主重算结果", () => {
  it("空账本即使收到范围完成证明，也不能申请完整停止", () => {
    const fixture = setup({ completenessPolicy: "scope_complete" });
    expect(fixture.decide({ completenessSatisfied: true, requestedStopReason: "complete" })).toEqual({
      shouldStop: false, status: null, stopReason: null, requestedStopAllowed: false,
    });
  });

  it("有真实支持的可选需求可以完成，不额外要求必须存在必需需求", () => {
    const fixture = setup();
    const need = fixture.need({ required: false });
    fixture.link(need.id);
    expect(fixture.decide({ requestedStopReason: "complete" })).toMatchObject({
      shouldStop: true, status: "completed", requestedStopAllowed: true,
    });
  });

  it("宿主接受不适用后免除该需求的来源、反证和单元要求", () => {
    const fixture = setup();
    const need = fixture.need({ minIndependentSources: 2, requireCounterEvidence: true, requireAllRelevantUnits: true });
    fixture.ledger.acceptNotApplicable(fixture.run.id, need.id, "宿主核对后确认此问题不适用于资料范围");
    expect(fixture.research.listEvidence(fixture.run.id)).toHaveLength(0);
    expect(fixture.decide({ requestedStopReason: "complete" })).toMatchObject({
      shouldStop: true, status: "completed", stopReason: "complete", requestedStopAllowed: true,
    });
  });

  it.each(["relevant_sections_complete", "scope_complete"] as const)("不适用需求不能代替 %s 的整个范围证明", completenessPolicy => {
    const fixture = setup({ completenessPolicy });
    const need = fixture.need({ requireAllRelevantUnits: true });
    fixture.ledger.acceptNotApplicable(fixture.run.id, need.id, "宿主确认此需求不适用");
    expect(fixture.decide({ requestedStopReason: "complete" }).shouldStop).toBe(false);
    expect(fixture.decide({ completenessSatisfied: true, requestedStopReason: "complete" })).toMatchObject({
      status: "completed", requestedStopAllowed: true,
    });
  });

  it("必需需求已支持仍须满足可选需求的独立来源要求", () => {
    const fixture = setup();
    const required = fixture.need();
    const optional = fixture.need({ required: false, minIndependentSources: 2 });
    fixture.link(required.id);
    fixture.link(optional.id);
    expect(fixture.ledger.recomputeNeed(fixture.run.id, optional.id).status).toBe("partial");
    expect(fixture.decide({ requestedStopReason: "complete" }).shouldStop).toBe(false);
    fixture.link(optional.id, 1);
    expect(fixture.decide({ requestedStopReason: "complete" }).status).toBe("completed");
  });

  it("单个需求的完整性证明与整个范围证明必须同时成立", () => {
    const fixture = setup();
    const need = fixture.need({ requireAllRelevantUnits: true });
    fixture.link(need.id);
    expect(fixture.decide({ completenessSatisfied: true, requestedStopReason: "complete" }).shouldStop).toBe(false);
    fixture.provenNeedIds.add(need.id);
    expect(fixture.ledger.recomputeNeed(fixture.run.id, need.id).status).toBe("supported");
    expect(fixture.decide({ requestedStopReason: "complete" }).shouldStop).toBe(false);
    expect(fixture.decide({ completenessSatisfied: true, requestedStopReason: "complete" }).status).toBe("completed");
  });

  it.each(["best_effort", "source_diverse"] as const)("普通 %s 需求不被未要求的范围证明阻塞", completenessPolicy => {
    const fixture = setup({ completenessPolicy });
    fixture.link(fixture.need().id);
    expect(fixture.decide({ completenessSatisfied: false, requestedStopReason: "complete" }).status).toBe("completed");
  });

  it("范围和需求均已检查完整，也不能抹去必需需求的冲突", () => {
    const fixture = setup({ completenessPolicy: "scope_complete" });
    const need = fixture.need({ requireCounterEvidence: true, requireAllRelevantUnits: true });
    fixture.link(need.id);
    fixture.link(need.id, 1, "contradicts");
    fixture.provenNeedIds.add(need.id);
    expect(fixture.ledger.recomputeNeed(fixture.run.id, need.id)).toMatchObject({
      status: "conflicted", completenessSatisfied: true,
    });
    expect(fixture.decide({ completenessSatisfied: true, requestedStopReason: "complete" }).shouldStop).toBe(false);
  });

  it("按本次持久化时限精确截止，已有完整证据也只能部分结束", () => {
    const fixture = setup({ budget: { maxWallClockMs: 17 } });
    fixture.link(fixture.need().id);
    expect(fixture.decide({ elapsedMs: 16, requestedStopReason: "budget_exhausted" }).shouldStop).toBe(false);
    expect(fixture.decide({ elapsedMs: 17, requestedStopReason: "complete" })).toMatchObject({
      shouldStop: true, status: "partial", stopReason: "wall_clock_exhausted", requestedStopAllowed: false,
    });
    expect(fixture.decide({ elapsedMs: 17, requestedStopReason: "budget_exhausted" }).requestedStopAllowed).toBe(true);
  });

  it("已完成轮数触达本次预算后不再批准完整，取消仍优先于轮数与时限", () => {
    const fixture = setup({ budget: { maxRounds: 2 } });
    fixture.link(fixture.need().id);
    fixture.finishRound(1);
    expect(fixture.decide({ requestedStopReason: "complete" }).status).toBe("completed");
    fixture.finishRound(0);
    expect(fixture.decide({ requestedStopReason: "complete" })).toMatchObject({
      shouldStop: true, status: "partial", stopReason: "round_budget_exhausted", requestedStopAllowed: false,
    });
    expect(fixture.decide({ elapsedMs: fixture.run.budget.maxWallClockMs, cancelled: true, requestedStopReason: "budget_exhausted" })).toEqual({
      shouldStop: true, status: "cancelled", stopReason: "cancelled", requestedStopAllowed: false,
    });
  });

  it("已有完整证据不替模型的假无进展理由兜底，真实连续空轮才批准该申请", () => {
    const fixture = setup();
    fixture.link(fixture.need().id);
    fixture.finishRound(1);
    fixture.finishRound(0);
    expect(fixture.decide({ requestedStopReason: "no_progress" })).toEqual({
      shouldStop: false, status: null, stopReason: null, requestedStopAllowed: false,
    });
    fixture.finishRound(0);
    expect(fixture.decide({ requestedStopReason: "no_progress" })).toMatchObject({
      shouldStop: true, status: "partial", stopReason: "no_progress", requestedStopAllowed: true,
    });
  });
});
