import { afterEach, describe, expect, it, vi } from "vitest";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { ResearchToolBudget, type KnowledgeResearchActorContext } from "../lib/knowledge/research/research-tool-budget.ts";
import { createKnowledgeResearchUpdateTool } from "../lib/tools/knowledge-research-update-tool.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const fixtures: ReturnType<typeof createKnowledgeResearchFixture>[] = [];
afterEach(() => { vi.restoreAllMocks(); for (const fixture of fixtures.splice(0)) fixture.close(); });
const needInput = { claim: "核对项目事实", kind: "fact" as const, required: true,
  minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false };

function setup() {
  const fixture = createKnowledgeResearchFixture(); fixtures.push(fixture);
  const ledger = new EvidenceLedger(fixture.research), budget = new ResearchToolBudget(fixture.research);
  const context: KnowledgeResearchActorContext = { runId: fixture.run.id, scopeId: fixture.scope.id,
    actorSessionId: "research-root-session", actorAgentId: "research-root", role: "root" };
  const tool = createKnowledgeResearchUpdateTool({ research: fixture.research, ledger, budget,
    resolveContext: ctx => ctx ? ctx as KnowledgeResearchActorContext : null });
  function receipt(index = 0) {
    const source = fixture.sources[index];
    return fixture.receipts.issue({ runId: fixture.run.id, actorSessionId: context.actorSessionId,
      sourceId: source.sourceId, contentSnapshotId: source.contentSnapshotId, parseArtifactId: source.parseArtifactId,
      blockId: source.blockId, startOffset: 0, endOffset: source.text.length, channel: "knowledge_read" });
  }
  const call = (params: Record<string, unknown>, actor: KnowledgeResearchActorContext | null = context, signal?: AbortSignal) =>
    tool.execute("update-test", { runId: fixture.run.id, ...params }, signal, undefined, actor);
  return { ...fixture, tool, ledger, budget, context, call, receipt };
}

describe("研究更新工具", () => {
  it("参数逐字段对应任务书，模型不能传最终状态或任意额外字段", () => {
    const { tool } = setup();
    expect(tool.name).toBe("knowledge_research_update");
    expect(tool.sessionPermission.resolveInvocation()).toEqual({ action: "read", kind: "read", capability: "knowledge_research_update.read" });
    expect(Object.keys(tool.parameters.properties)).toEqual(["runId", "createNeeds", "linkEvidence", "unresolvedGaps", "requestCompletenessPolicy"]);
    expect(tool.parameters.required).toEqual(["runId"]);
    expect(tool.parameters).toHaveProperty("additionalProperties", false);
    const needs = tool.parameters.properties.createNeeds.items;
    expect(Object.keys(needs.properties)).toEqual(["claim", "kind", "required", "minIndependentSources", "requireCounterEvidence", "requireAllRelevantUnits"]);
    expect(needs.properties.kind.anyOf.map(option => option.const)).toEqual(["fact", "comparison", "cause", "timeline", "counterexample", "completeness"]);
    expect(needs).toHaveProperty("additionalProperties", false);
    expect(Object.keys(tool.parameters.properties.linkEvidence.items.properties)).toEqual(["needId", "receiptId", "quote", "occurrenceIndex", "relation", "rationale"]);
    expect(Object.keys(tool.parameters.properties.unresolvedGaps.items.properties)).toEqual(["needId", "gaps"]);
    expect(tool.parameters.properties.requestCompletenessPolicy.anyOf.map(option => option.const))
      .toEqual(["source_diverse", "relevant_sections_complete", "scope_complete"]);
  });

  it("根会话创建需求并升级策略，状态由宿主计算；动作只记编号与计数", async () => {
    const f = setup();
    const result = await f.call({ createNeeds: [{ ...needInput, claim: "甲".repeat(1000) }, { ...needInput, kind: "counterexample" }],
      requestCompletenessPolicy: "relevant_sections_complete" });
    expect(result.isError).toBeUndefined();
    const payload = JSON.parse(result.content[0].text);
    expect(payload).toMatchObject({ runId: f.run.id, completenessPolicy: "relevant_sections_complete",
      needs: [{ ordinal: 0, status: "uncovered", evidenceIds: [] }, { ordinal: 1, status: "uncovered", evidenceIds: [] }] });
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(1);
    const actions = f.research.listActions(f.run.id);
    expect(actions).toHaveLength(1);
    expect(actions[0]).toMatchObject({ actionType: "knowledge_research_update", status: "completed",
      requestSummary: { needIds: [] }, responseSummary: { count: 2, status: "completed" } });
    expect(JSON.stringify(actions)).not.toContain("甲".repeat(1000));
  });

  it("单运行最多八项需求，第九项及批次中途超限均拒绝且原子回滚", async () => {
    const f = setup();
    for (let index = 0; index < 7; index++) f.research.createNeed(f.run.id, { ...needInput, claim: `已有需求${index}` });
    const failed = await f.call({ createNeeds: [needInput, needInput] });
    expect(failed.isError).toBe(true);
    expect(failed.details).toMatchObject({ errorCode: "KNOWLEDGE_CONFLICT" });
    expect(f.research.listNeeds(f.run.id)).toHaveLength(7);
    expect(f.research.listActions(f.run.id)[0].status).toBe("failed");
    expect((await f.call({ createNeeds: [needInput] })).isError).toBeUndefined();
    expect(f.research.listNeeds(f.run.id)).toHaveLength(8);
    expect(() => f.research.createNeed(f.run.id, needInput)).toThrow(/at most eight/);
    expect((await f.call({ createNeeds: [needInput] })).isError).toBe(true);
    expect(f.research.listNeeds(f.run.id)).toHaveLength(8);
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(3);
  });

  it("引用交台账核验，允许最大理由和缺口长度，最终状态不能由缺口参数冒充", async () => {
    const f = setup(); const need = f.research.createNeed(f.run.id, needInput); const receipt = f.receipt();
    const result = await f.call({ linkEvidence: [{ needId: need.id, receiptId: receipt.id, quote: "九月十五日",
      relation: "supports", rationale: "证".repeat(1000) }], unresolvedGaps: [{ needId: need.id, gaps: Array(8).fill("缺".repeat(500)) }] });
    expect(result.isError).toBeUndefined();
    expect(JSON.parse(result.content[0].text).needs[0]).toMatchObject({ status: "supported", independentSourceCount: 1,
      unresolvedGaps: Array(8).fill("缺".repeat(500)) });
    expect(f.research.listEvidence(f.run.id)[0].canonicalText).toBe("九月十五日");
    expect(f.research.getReceipt(f.run.id, receipt.id).consumedAt).not.toBeNull();
    expect(f.research.listActions(f.run.id)[0].requestSummary).toEqual({ needIds: [need.id] });
    expect(JSON.stringify(f.research.listActions(f.run.id))).not.toContain("九月十五日");
  });

  it("批次后一条伪造引用失败时，前面的需求、证据、关联与凭据消费全部回滚", async () => {
    const f = setup(); const need = f.research.createNeed(f.run.id, needInput); const receipt = f.receipt();
    const link = { needId: need.id, receiptId: receipt.id, quote: "九月十五日", relation: "supports", rationale: "资料明确说明" };
    const result = await f.call({ createNeeds: [needInput], linkEvidence: [link, { ...link, quote: "不存在的日期" }],
      requestCompletenessPolicy: "scope_complete" });
    expect(result.details).toMatchObject({ errorCode: "KNOWLEDGE_MODEL_OUTPUT_INVALID" });
    expect(f.research.listNeeds(f.run.id)).toHaveLength(1);
    expect(f.research.getNeed(f.run.id, need.id).status).toBe("uncovered");
    expect(f.research.listEvidence(f.run.id)).toEqual([]);
    expect(f.research.listRelations(f.run.id)).toEqual([]);
    expect(f.research.getReceipt(f.run.id, receipt.id).consumedAt).toBeNull();
    expect(f.research.requireRun(f.run.id).completenessPolicy).toBe("source_diverse");
    expect(f.research.listActions(f.run.id)[0]).toMatchObject({ status: "failed", errorCode: "KNOWLEDGE_MODEL_OUTPUT_INVALID" });
  });

  it("完整性只能升级，相同策略可重试，降级会回滚同批证据与缺口更新", async () => {
    const f = setup(); const need = f.research.createNeed(f.run.id, needInput); const receipt = f.receipt();
    expect((await f.call({ requestCompletenessPolicy: "scope_complete" })).isError).toBeUndefined();
    expect((await f.call({ requestCompletenessPolicy: "scope_complete" })).isError).toBeUndefined();
    const result = await f.call({ createNeeds: [needInput], linkEvidence: [{ needId: need.id, receiptId: receipt.id,
      quote: "九月十五日", relation: "supports", rationale: "原文给出日期" }],
      unresolvedGaps: [{ needId: need.id, gaps: ["新增缺口"] }], requestCompletenessPolicy: "source_diverse" });
    expect(result.details).toMatchObject({ errorCode: "KNOWLEDGE_CONFLICT" });
    expect(f.research.requireRun(f.run.id).completenessPolicy).toBe("scope_complete");
    expect(f.research.listNeeds(f.run.id)).toHaveLength(1);
    expect(f.research.getNeed(f.run.id, need.id)).toMatchObject({ status: "uncovered", unresolvedGaps: [] });
    expect(f.research.listEvidence(f.run.id)).toEqual([]);
    expect(f.research.getReceipt(f.run.id, receipt.id).consumedAt).toBeNull();
  });

  it("所有越限、错误类型及未知字段明确拒绝，已授权失败仍消耗工具预算", async () => {
    const f = setup(); const need = f.research.createNeed(f.run.id, needInput); const receipt = f.receipt();
    const link = { needId: need.id, receiptId: receipt.id, quote: "九月十五日", relation: "supports", rationale: "核验原文" };
    const bad = [
      { status: "supported" }, { createNeeds: [{ ...needInput, status: "supported" }] },
      { createNeeds: [{ ...needInput, claim: "甲".repeat(1001) }] }, { createNeeds: [{ ...needInput, kind: "unknown" }] },
      { createNeeds: [{ ...needInput, kind: ["fact"] }] },
      { createNeeds: [{ ...needInput, required: "true" }] }, { createNeeds: [{ ...needInput, minIndependentSources: 0 }] },
      { createNeeds: [{ ...needInput, minIndependentSources: 1.5 }] }, { createNeeds: null },
      { linkEvidence: [{ ...link, rationale: "证".repeat(1001) }] }, { linkEvidence: [{ ...link, quote: "文".repeat(2001) }] },
      { linkEvidence: [{ ...link, occurrenceIndex: -1 }] }, { linkEvidence: [{ ...link, sourceId: f.sources[0].sourceId }] },
      { linkEvidence: [{ ...link, relation: ["supports"] }] },
      { unresolvedGaps: [{ needId: need.id, gaps: Array(9).fill("缺口") }] },
      { unresolvedGaps: [{ needId: need.id, gaps: ["缺".repeat(501)] }] },
      { unresolvedGaps: [{ needId: need.id, gaps: ["缺口"], status: "supported" }] },
      { requestCompletenessPolicy: "best_effort" },
      { requestCompletenessPolicy: ["scope_complete"] },
    ];
    for (const params of bad) expect((await f.call(params)).details).toMatchObject({ errorCode: "KNOWLEDGE_INVALID_ARGUMENT" });
    expect(f.research.listNeeds(f.run.id)).toHaveLength(1);
    expect(f.research.listEvidence(f.run.id)).toEqual([]);
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(bad.length);
    expect(f.research.listActions(f.run.id)).toHaveLength(bad.length);
    expect(f.research.listActions(f.run.id).every(action => action.status === "failed")).toBe(true);
  });

  it("Worker只更新分配需求和来源，不新增需求、修改全局策略或返回其他需求", async () => {
    const f = setup(); const assigned = f.research.createNeed(f.run.id, needInput), other = f.research.createNeed(f.run.id, needInput);
    const receipt = f.receipt(), outside = f.receipt(1);
    const context: KnowledgeResearchActorContext = { ...f.context, role: "worker", actorSessionId: "worker-session", actorAgentId: "worker",
      allowedNeedIds: [assigned.id], allowedSourceIds: [f.sources[0].sourceId] };
    const link = { needId: assigned.id, receiptId: receipt.id, quote: "九月十五日", relation: "supports", rationale: "原文事实" };
    for (const params of [{ createNeeds: [needInput] }, { requestCompletenessPolicy: "scope_complete" },
      { unresolvedGaps: [{ needId: other.id, gaps: ["越权缺口"] }] }, { linkEvidence: [{ ...link, needId: other.id }] },
      { linkEvidence: [{ ...link, receiptId: outside.id, quote: "三十二万元" }] }]) {
      expect((await f.call(params, context)).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    }
    const result = await f.call({ linkEvidence: [link], unresolvedGaps: [{ needId: assigned.id, gaps: ["仍待核对其他事项"] }] }, context);
    expect(result.isError).toBeUndefined();
    expect(JSON.parse(result.content[0].text).needs.map((need: { id: string }) => need.id)).toEqual([assigned.id]);
    expect(f.research.getNeed(f.run.id, other.id)).toMatchObject({ status: "uncovered", unresolvedGaps: [] });
    expect(f.research.listNeeds(f.run.id)).toHaveLength(2);
  });

  it("未授权上下文、伪造run和scope在预算及修改之前拒绝", async () => {
    const f = setup();
    for (const context of [null, { ...f.context, runId: "another-run" }, { ...f.context, scopeId: "another-scope" }]) {
      expect((await f.call({ createNeeds: [needInput] }, context)).isError).toBe(true);
    }
    expect((await f.call({ runId: "another-run", createNeeds: [needInput] })).isError).toBe(true);
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(0);
    expect(f.research.listNeeds(f.run.id)).toEqual([]);
    expect(f.research.listActions(f.run.id)).toEqual([]);
  });

  it("取消信号向外传播，事务内已完成的引用写入也会回滚", async () => {
    const f = setup(), controller = new AbortController();
    const need = f.research.createNeed(f.run.id, needInput), receipt = f.receipt();
    const original = f.ledger.linkEvidence.bind(f.ledger);
    vi.spyOn(f.ledger, "linkEvidence").mockImplementation((...args) => {
      const result = original(...args); controller.abort(); return result;
    });
    await expect(f.call({ linkEvidence: [{ needId: need.id, receiptId: receipt.id, quote: "九月十五日",
      relation: "supports", rationale: "真实原文" }] }, f.context, controller.signal)).rejects.toMatchObject({ name: "AbortError" });
    expect(f.research.listEvidence(f.run.id)).toEqual([]);
    expect(f.research.getReceipt(f.run.id, receipt.id).consumedAt).toBeNull();
    expect(f.research.listActions(f.run.id)[0].status).toBe("cancelled");
    expect(f.research.requireRun(f.run.id)).toMatchObject({ status: "cancelled", stopReason: "cancelled" });
  });
});

describe("已开始研究动作的持久化收尾", () => {
  it("运行终止后仍能收尾在途动作，相同结果幂等但不允许改写其他终态", () => {
    const f = setup();
    const action = f.research.insertAction({ id: "action-in-flight", runId: f.run.id, roundId: null, ordinal: 0,
      actorSessionId: f.context.actorSessionId, actorAgentId: f.context.actorAgentId, actionType: "knowledge_research_update",
      requestSummary: { needIds: [] }, responseSummary: null, status: "running", startedAt: f.research.now(), completedAt: null, errorCode: null });
    f.store.db.prepare("UPDATE knowledge_research_runs SET status = 'cancelled' WHERE id = ?").run(f.run.id);
    const result = f.research.finishAction(f.run.id, action.id, { status: "cancelled", responseSummary: { count: 0, status: "cancelled" }, errorCode: "CANCELLED" });
    expect(result).toMatchObject({ status: "cancelled", errorCode: "CANCELLED" });
    expect(result.completedAt).not.toBeNull();
    expect(f.research.finishAction(f.run.id, action.id, { status: "cancelled", responseSummary: { status: "cancelled", count: 0 }, errorCode: "CANCELLED" })).toEqual(result);
    expect(() => f.research.finishAction(f.run.id, action.id, { status: "completed", responseSummary: { count: 1 }, errorCode: null })).toThrow(/different terminal result/);
    expect(() => f.research.finishAction(f.run.id, "missing", { status: "failed", responseSummary: null, errorCode: "FAILED" })).toThrow(/not found/);
  });

  it("动作收尾不能借响应摘要写正文，跨运行不能收尾其他动作", () => {
    const f = setup();
    const action = f.research.insertAction({ id: "action-in-flight", runId: f.run.id, roundId: null, ordinal: 0,
      actorSessionId: null, actorAgentId: null, actionType: "knowledge_research_update", requestSummary: {}, responseSummary: null,
      status: "running", startedAt: f.research.now(), completedAt: null, errorCode: null });
    expect(() => f.research.finishAction(f.run.id, action.id, { status: "completed", responseSummary: { rawAnswer: "不允许保存" }, errorCode: null }))
      .toThrow(/metadata is invalid/);
    const other = f.research.createRun({ turnScopeId: f.scope.id, turnId: f.scope.turnId, parentSessionPath: f.scope.sessionPath, question: "另一研究" });
    expect(() => f.research.finishAction(other.id, action.id, { status: "completed", responseSummary: null, errorCode: null })).toThrow(/not found/);
    expect(f.research.listActions(f.run.id)[0]).toMatchObject({ status: "running", completedAt: null });
  });
});
