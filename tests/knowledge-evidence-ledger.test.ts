import { afterEach, describe, expect, it } from "vitest";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { evaluateResearchStopPolicy } from "../lib/knowledge/research/research-stop-policy.ts";
import type { KnowledgeEvidenceNeedRecord, KnowledgeResearchAction } from "../lib/knowledge/types.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const fixtures: ReturnType<typeof createKnowledgeResearchFixture>[] = [];
afterEach(() => { for (const f of fixtures.splice(0)) f.close(); });
function setup() {
  const f = createKnowledgeResearchFixture(["交付日期是九月十五日。预算三十二万。", "公告说交付九月二十日。解释是新版计划延期。"]); fixtures.push(f);
  const ledger = new EvidenceLedger(f.research);
  const need = (options: Partial<Pick<KnowledgeEvidenceNeedRecord, "minIndependentSources" | "requireCounterEvidence" | "requireAllRelevantUnits" | "required">> = {}) =>
    f.research.createNeed(f.run.id, { claim: "确认交付日期", kind: "fact", required: true,
      minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false, ...options });
  const link = (needId: string, sourceIndex: number, quote: string, relation: "supports" | "contradicts" | "context" = "supports") => {
    const source = f.sources[sourceIndex];
    const receipt = f.receipts.issue({ runId: f.run.id, actorSessionId: null, ...source,
      startOffset: 0, endOffset: source.text.length, channel: "knowledge_read" });
    return ledger.linkEvidence({ runId: f.run.id, needId, receiptId: receipt.id, quote, relation, rationale: "原文说明此事实" });
  };
  const searchAction = (needId: string, overrides: Partial<KnowledgeResearchAction> = {}) => {
    const actions = f.research.listActions(f.run.id);
    f.research.insertAction({ id: f.research.newId("act"), runId: f.run.id, roundId: null,
      ordinal: actions.length, actorSessionId: null, actorAgentId: null, actionType: "knowledge_search",
      requestSummary: { query: "日期变更的反例", purpose: "counterexample", needIds: [needId] },
      responseSummary: { count: 0, hitIds: [] }, status: "completed", startedAt: f.research.now(),
      completedAt: f.research.now(), errorCode: null, ...overrides });
  };
  return { ...f, ledger, need, link, searchAction };
}

describe("研究证据台账由宿主计算", () => {
  it("空需求和只有背景关系保持未覆盖，不接受数据库中自报的已支持", () => {
    const f = setup(); const need = f.need();
    expect(f.ledger.recomputeNeed(f.run.id, need.id).status).toBe("uncovered");
    expect(f.link(need.id, 0, "预算三十二万", "context").need.status).toBe("uncovered");
    f.store.db.prepare("UPDATE knowledge_evidence_needs SET status='supported' WHERE id=?").run(need.id);
    expect(f.ledger.recomputeNeed(f.run.id, need.id).status).toBe("uncovered");
  });

  it("同源不同片段仍只算一个来源，两个独立来源才满足要求", () => {
    const f = setup(); const need = f.need({ minIndependentSources: 2 });
    expect(f.link(need.id, 0, "交付日期是九月十五日").need.status).toBe("partial");
    const sameSource = f.link(need.id, 0, "预算三十二万");
    expect(sameSource.need.independentSourceCount).toBe(1);
    const independent = f.link(need.id, 1, "公告说交付九月二十日");
    expect(independent.need.independentSourceCount).toBe(2);
    expect(independent.need.status).toBe("supported");
    expect(f.research.listRelations(f.run.id).map(item => item.sourceIndependenceKey).sort())
      .toEqual([f.sources[0].sourceId, f.sources[0].sourceId, f.sources[1].sourceId].sort());
  });

  it("支持与反证同时存在必须冲突，不能因反证数量满足而改成已支持", () => {
    const f = setup(); const need = f.need({ requireCounterEvidence: true });
    f.link(need.id, 0, "交付日期是九月十五日");
    const result = f.link(need.id, 1, "公告说交付九月二十日", "contradicts");
    expect(result.need.status).toBe("conflicted");
    expect(result.need.counterEvidenceChecked).toBe(false);
    expect(result.need.counterEvidenceIds).toEqual([result.evidence.id]);
  });

  it("没有反证不代表检查完成，普通零命中查询与其他需求查询均不算", () => {
    const f = setup(); const need = f.need({ requireCounterEvidence: true }); const other = f.need();
    expect(f.link(need.id, 0, "交付日期是九月十五日").need.status).toBe("partial");
    f.searchAction(need.id, { requestSummary: { query: "交付日期", needIds: [need.id] } });
    f.searchAction(other.id);
    f.searchAction(need.id, { status: "failed", errorCode: "SEARCH_FAILED" });
    f.searchAction(need.id, { responseSummary: { count: 1, hitIds: [f.sources[1].blockId] } });
    expect(f.ledger.recomputeNeed(f.run.id, need.id).counterEvidenceChecked).toBe(false);
    f.searchAction(need.id);
    const result = f.ledger.recomputeNeed(f.run.id, need.id);
    expect(result.status).toBe("supported");
    expect(result.counterEvidenceChecked).toBe(true);
  });

  it("带错误或失败响应的零命中动作不能冒充反证检查成功", () => {
    const f = setup(); const need = f.need({ requireCounterEvidence: true });
    f.link(need.id, 0, "交付日期是九月十五日");
    const failed: Partial<KnowledgeResearchAction>[] = [
      { errorCode: "SEARCH_UNAVAILABLE" },
      { responseSummary: { count: 0, hitIds: [], errorCode: "SEARCH_UNAVAILABLE" } },
      { responseSummary: { count: 0, hitIds: [], status: "failed" } },
      { completedAt: null },
    ];
    for (const action of failed) {
      f.searchAction(need.id, action);
      expect(f.ledger.recomputeNeed(f.run.id, need.id)).toMatchObject({ status: "partial", counterEvidenceChecked: false });
    }
  });

  it("专门零命中查询不能抹掉已经存在的矛盾", () => {
    const f = setup(); const need = f.need({ requireCounterEvidence: true });
    f.link(need.id, 0, "交付日期是九月十五日");
    f.link(need.id, 1, "公告说交付九月二十日", "contradicts");
    f.searchAction(need.id);
    expect(f.ledger.recomputeNeed(f.run.id, need.id).status).toBe("conflicted");
  });

  it("新解释需求有支持、关联反证并经宿主接受后才解除冲突，原反证仍保留", () => {
    const f = setup(); const need = f.need({ requireCounterEvidence: true });
    f.link(need.id, 0, "交付日期是九月十五日");
    const counter = f.link(need.id, 1, "公告说交付九月二十日", "contradicts");
    const explanation = f.need();
    expect(() => f.ledger.acceptConflictResolution(f.run.id, need.id, explanation.id)).toThrow();
    f.link(explanation.id, 1, "解释是新版计划延期");
    expect(() => f.ledger.acceptConflictResolution(f.run.id, need.id, explanation.id)).toThrow();
    f.link(explanation.id, 1, "公告说交付九月二十日", "context");
    expect(f.ledger.recomputeNeed(f.run.id, need.id).status).toBe("conflicted");
    const resolved = f.ledger.acceptConflictResolution(f.run.id, need.id, explanation.id);
    expect(resolved.status).toBe("supported");
    expect(resolved.counterEvidenceChecked).toBe(true);
    expect(resolved.counterEvidenceIds).toEqual([counter.evidence.id]);
    expect(new EvidenceLedger(f.research).evaluateNeed(f.run.id, need.id).status).toBe("supported");
  });

  it("后来的新反证没有被解释时重新进入冲突，过去的接受不能一劳永逸", () => {
    const f = setup(); const need = f.need();
    f.link(need.id, 0, "交付日期是九月十五日");
    f.link(need.id, 1, "公告说交付九月二十日", "contradicts");
    const explanation = f.need();
    f.link(explanation.id, 1, "解释是新版计划延期");
    f.link(explanation.id, 1, "公告说交付九月二十日", "context");
    f.ledger.acceptConflictResolution(f.run.id, need.id, explanation.id);
    expect(f.link(need.id, 0, "预算三十二万", "contradicts").need.status).toBe("conflicted");
  });

  it("完整性证明由宿主提供，缺少执行器时保持部分支持", () => {
    const f = setup(); const need = f.need({ requireCounterEvidence: true, requireAllRelevantUnits: true });
    f.link(need.id, 0, "交付日期是九月十五日");
    expect(f.ledger.recomputeNeed(f.run.id, need.id).status).toBe("partial");
    const checked = new EvidenceLedger(f.research, { isCompletenessSatisfied: (runId, needId) => runId === f.run.id && needId === need.id });
    expect(checked.recomputeNeed(f.run.id, need.id)).toMatchObject({ status: "supported", counterEvidenceChecked: true, completenessSatisfied: true });
    expect(f.ledger.recomputeNeed(f.run.id, need.id).status).toBe("partial");
  });

  it("只有宿主明确接受才是不适用，模型或持久化状态自报不起作用", () => {
    const f = setup(); const need = f.need({ minIndependentSources: 2, requireCounterEvidence: true });
    f.store.db.prepare("UPDATE knowledge_evidence_needs SET status='not_applicable' WHERE id=?").run(need.id);
    expect(f.ledger.recomputeNeed(f.run.id, need.id).status).toBe("uncovered");
    expect(() => f.ledger.acceptNotApplicable(f.run.id, need.id, "")).toThrow();
    expect(f.ledger.acceptNotApplicable(f.run.id, need.id, "宿主确认此需求不适用于该问题").status).toBe("not_applicable");
    expect(new EvidenceLedger(f.research).evaluateNeed(f.run.id, need.id).status).toBe("not_applicable");
  });

  it("停止判断消费重算结果，不能凭部分支持或伪称预算用尽结束", () => {
    const f = setup(); const need = f.need({ requireCounterEvidence: true });
    f.link(need.id, 0, "交付日期是九月十五日");
    const input = { run: f.run, needs: f.ledger.recompute(f.run.id), elapsedMs: 1,
      recentRoundEvidenceCounts: [1], completenessSatisfied: false };
    expect(evaluateResearchStopPolicy({ ...input, requestedStopReason: "complete" }).shouldStop).toBe(false);
    expect(evaluateResearchStopPolicy({ ...input, requestedStopReason: "budget_exhausted" }).shouldStop).toBe(false);
    expect(evaluateResearchStopPolicy({ ...input, requestedStopReason: "no_progress" }).shouldStop).toBe(false);
    f.searchAction(need.id);
    expect(evaluateResearchStopPolicy({ ...input, needs: f.ledger.recompute(f.run.id), requestedStopReason: "complete" }))
      .toMatchObject({ shouldStop: true, status: "completed", stopReason: "complete", requestedStopAllowed: true });
    expect(evaluateResearchStopPolicy({ ...input, needs: f.ledger.recompute(f.run.id),
      run: { ...f.run, toolCallsUsed: 32 }, requestedStopReason: "complete" }))
      .toMatchObject({ shouldStop: true, status: "partial", stopReason: "tool_budget_exhausted", requestedStopAllowed: false });
    expect(evaluateResearchStopPolicy({ ...input, run: { ...f.run, toolCallsUsed: 32 } }))
      .toMatchObject({ shouldStop: true, status: "partial", stopReason: "tool_budget_exhausted" });
    expect(evaluateResearchStopPolicy({ ...input, run: { ...f.run, roundsCompleted: 2 }, recentRoundEvidenceCounts: [1, 0, 0] }))
      .toMatchObject({ shouldStop: true, status: "partial", stopReason: "no_progress" });
    expect(evaluateResearchStopPolicy({ ...input, cancelled: true }))
      .toMatchObject({ shouldStop: true, status: "cancelled", stopReason: "cancelled" });
  });
});
