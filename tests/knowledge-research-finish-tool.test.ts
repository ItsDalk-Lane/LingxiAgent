import { afterEach, describe, expect, it, vi } from "vitest";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { ResearchToolBudget, type KnowledgeResearchActorContext } from "../lib/knowledge/research/research-tool-budget.ts";
import type { KnowledgeEvidenceNeedRecord } from "../lib/knowledge/types.ts";
import { createKnowledgeResearchFinishTool } from "../lib/tools/knowledge-research-finish-tool.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const fixtures: ReturnType<typeof createKnowledgeResearchFixture>[] = [];
afterEach(() => { for (const fixture of fixtures.splice(0)) fixture.close(); });

function setup(options: { complete?: () => boolean } = {}) {
  const fixture = createKnowledgeResearchFixture();
  fixtures.push(fixture);
  const ledger = new EvidenceLedger(fixture.research, { isCompletenessSatisfied: () => options.complete?.() === true });
  const budget = new ResearchToolBudget(fixture.research);
  const context: KnowledgeResearchActorContext = { runId: fixture.run.id, scopeId: fixture.scope.id, actorSessionId: "research-root",
    actorAgentId: "research-agent", role: "root" };
  const onFinishAccepted = vi.fn();
  const deps = { research: fixture.research, ledger, budget, resolveContext: () => context,
    isCompletenessSatisfied: () => options.complete?.() === true, onFinishAccepted };
  const tool = createKnowledgeResearchFinishTool(deps);
  const need = (overrides: Partial<Pick<KnowledgeEvidenceNeedRecord,
    "minIndependentSources" | "requireCounterEvidence" | "requireAllRelevantUnits">> = {}) =>
    fixture.research.createNeed(fixture.run.id, { claim: "确认项目事实", kind: "fact", required: true,
      minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false, ...overrides });
  const support = (needId: string, sourceIndex = 0, quote = fixture.sources[sourceIndex].text) => {
    const source = fixture.sources[sourceIndex];
    const receipt = fixture.receipts.issue({ runId: fixture.run.id, actorSessionId: context.actorSessionId,
      ...source, startOffset: 0, endOffset: source.text.length, channel: "knowledge_read" });
    return ledger.linkEvidence({ runId: fixture.run.id, needId, receiptId: receipt.id, quote,
      relation: "supports", rationale: "冻结原文说明此项事实" });
  };
  const finish = (requestedStopReason = "complete", params: Record<string, unknown> = {}, signal?: AbortSignal) =>
    tool.execute("finish-call", { runId: fixture.run.id, conclusionSummary: "研究结论摘要", requestedStopReason, ...params }, signal);
  const rounds = (counts: number[], runningLast = false) => {
    const now = fixture.research.now();
    counts.forEach((count, ordinal) => {
      const running = runningLast && ordinal === counts.length - 1;
      fixture.store.db.prepare(`INSERT INTO knowledge_research_rounds
        (id, run_id, ordinal, focus_json, status, new_evidence_count, started_at, completed_at)
        VALUES (?, ?, ?, '[]', ?, ?, ?, ?)`).run(`round-${ordinal}`, fixture.run.id, ordinal,
        running ? "running" : "completed", count, now, running ? null : now);
    });
    fixture.store.db.prepare("UPDATE knowledge_research_runs SET rounds_completed = ? WHERE id = ?")
      .run(counts.length - Number(runningLast), fixture.run.id);
  };
  return { ...fixture, ledger, budget, context, deps, tool, need, support, finish, rounds, onFinishAccepted };
}

function payload(result: { content: Array<{ text: string }> }) { return JSON.parse(result.content[0].text); }

describe("研究结束申请只能由宿主核准", () => {
  it("空需求和数据库自报已支持均不能使完整申请获准，拒绝仍记工具次数", async () => {
    const f = setup();
    expect(payload(await f.finish())).toMatchObject({ accepted: false, requestedStopReason: "complete", status: null });
    const need = f.need();
    f.store.db.prepare("UPDATE knowledge_evidence_needs SET status = 'supported' WHERE id = ?").run(need.id);
    expect(payload(await f.finish())).toMatchObject({ accepted: false, status: null });
    expect(f.research.getNeed(f.run.id, need.id).status).toBe("uncovered");
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(2);
    expect(f.research.listActions(f.run.id).map(action => action.responseSummary?.status)).toEqual(["rejected", "rejected"]);
    expect(f.onFinishAccepted).not.toHaveBeenCalled();
  });

  it("有效来源满足需求后传递宿主接受结果，不直接把研究标为最终完成", async () => {
    const f = setup(); const need = f.need(); f.support(need.id);
    const result = payload(await f.finish());
    expect(result).toEqual({ runId: f.run.id, accepted: true, requestedStopReason: "complete",
      stopReason: "complete", status: "completed", remainingBudget: { shared: true, toolCalls: 31, wallClockMs: expect.any(Number) } });
    const { remainingBudget, ...decision } = result;
    expect(remainingBudget).toMatchObject({ shared: true, toolCalls: 31 });
    expect(f.onFinishAccepted).toHaveBeenCalledExactlyOnceWith(decision, f.context);
    expect(f.research.requireRun(f.run.id).status).not.toBe("completed");
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(1);
  });

  it("同一来源的两段证据不能冒充两个独立来源", async () => {
    const f = setup(); const need = f.need({ minIndependentSources: 2 });
    f.support(need.id); f.support(need.id, 0, "九月十五日");
    expect(payload(await f.finish()).accepted).toBe(false);
    f.support(need.id, 1);
    expect(payload(await f.finish()).accepted).toBe(true);
  });

  it("模型声称没有反例不能替代反证检查，宿主成功零命中记录才算完成", async () => {
    const f = setup(); const need = f.need({ requireCounterEvidence: true }); f.support(need.id);
    expect(payload(await f.finish("complete", { conclusionSummary: "已检查全部反例，没有发现任何问题" })).accepted).toBe(false);
    const now = f.research.now();
    f.research.insertAction({ id: f.research.newId("action"), runId: f.run.id, roundId: null, ordinal: 1,
      actorSessionId: f.context.actorSessionId, actorAgentId: f.context.actorAgentId, actionType: "knowledge_search",
      requestSummary: { query: "项目延期的反例", purpose: "counterexample", needIds: [need.id] },
      responseSummary: { count: 0, hitIds: [] }, status: "completed", startedAt: now, completedAt: now, errorCode: null });
    expect(payload(await f.finish()).accepted).toBe(true);
  });

  it("完整范围要求必须有宿主证明，结论摘要不能自行宣告已检查全部资料", async () => {
    let complete = false;
    const f = setup({ complete: () => complete }); const need = f.need({ requireAllRelevantUnits: true });
    f.store.db.prepare("UPDATE knowledge_research_runs SET completeness_policy = 'scope_complete' WHERE id = ?").run(f.run.id);
    f.support(need.id);
    expect(payload(await f.finish("complete", { conclusionSummary: "全范围已完整阅读" })).accepted).toBe(false);
    complete = true;
    expect(payload(await f.finish()).accepted).toBe(true);
  });

  it("没有实际耗尽预算时拒绝预算申请，不因结论文字而提前结束", async () => {
    const f = setup(); f.need();
    expect(payload(await f.finish("budget_exhausted", { conclusionSummary: "预算肯定已耗尽" }))).toMatchObject({
      accepted: false, requestedStopReason: "budget_exhausted", stopReason: null, status: null,
    });
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(1);
    expect(f.onFinishAccepted).not.toHaveBeenCalled();
  });

  it("实际完成最大轮数时允许按预算部分结束", async () => {
    const f = setup(); f.need(); f.rounds([1, 1, 1, 1]);
    const result = payload(await f.finish("budget_exhausted"));
    expect(result).toMatchObject({ accepted: true, stopReason: "round_budget_exhausted", status: "partial" });
    const { remainingBudget, ...decision } = result;
    expect(remainingBudget).toMatchObject({ shared: true, toolCalls: 31 });
    expect(f.onFinishAccepted).toHaveBeenCalledExactlyOnceWith(decision, f.context);
  });

  it.each(["complete", "budget_exhausted"])("第32次调用申请 %s 时只能按真实工具预算部分结束", async (reason) => {
    const f = setup(); const need = f.need(); f.support(need.id);
    f.store.db.prepare("UPDATE knowledge_research_runs SET tool_calls_used = 31 WHERE id = ?").run(f.run.id);
    const result = payload(await f.finish(reason));
    expect(result).toMatchObject({ accepted: reason === "budget_exhausted", status: "partial", stopReason: "tool_budget_exhausted" });
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 32, status: "partial", stopReason: "tool_budget_exhausted" });
    expect(f.onFinishAccepted).toHaveBeenCalledTimes(Number(reason === "budget_exhausted"));
  });

  it("已经用完工具预算时不再创建第33次动作，也不伪造结束申请获准", async () => {
    const f = setup(); f.need();
    f.store.db.prepare("UPDATE knowledge_research_runs SET tool_calls_used = 32 WHERE id = ?").run(f.run.id);
    expect((await f.finish("budget_exhausted")).details).toMatchObject({ errorCode: "KNOWLEDGE_CONFLICT" });
    expect(f.research.listActions(f.run.id)).toEqual([]);
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 32, status: "partial", stopReason: "tool_budget_exhausted" });
    expect(f.onFinishAccepted).not.toHaveBeenCalled();
  });

  it.each([{ counts: [0] }, { counts: [0, 1, 0] }])("实际轮次 $counts 不满足连续两轮零新增", async ({ counts }) => {
    const f = setup(); f.need(); f.rounds(counts);
    expect(payload(await f.finish("no_progress"))).toMatchObject({ accepted: false, status: null });
    expect(f.onFinishAccepted).not.toHaveBeenCalled();
  });

  it("未完成的当前轮不算已经连续两轮没有进展", async () => {
    const f = setup(); f.need(); f.rounds([0, 0], true);
    expect(payload(await f.finish("no_progress"))).toMatchObject({ accepted: false, status: null });
  });

  it("真实完成轮次的末两轮均为零新增时允许部分结束", async () => {
    const f = setup(); f.need(); f.rounds([1, 0, 0]);
    const result = payload(await f.finish("no_progress"));
    expect(result).toMatchObject({ accepted: true, stopReason: "no_progress", status: "partial" });
    const { remainingBudget, ...decision } = result;
    expect(remainingBudget).toMatchObject({ shared: true, toolCalls: 31 });
    expect(f.onFinishAccepted).toHaveBeenCalledExactlyOnceWith(decision, f.context);
    expect(f.research.requireRun(f.run.id).status).not.toBe("completed");
  });

  it("工作会话及跨研究请求均拒绝，不能获得结束回调", async () => {
    const f = setup(); const need = f.need(); f.support(need.id);
    f.context.role = "worker";
    f.context.allowedNeedIds = [need.id];
    expect((await f.finish()).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    f.context.role = "root";
    const other = f.research.createRun({ turnScopeId: f.scope.id, turnId: f.scope.turnId,
      parentSessionPath: f.scope.sessionPath, question: "另一项研究" });
    expect((await f.finish("complete", { runId: other.id })).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(0);
    expect(f.onFinishAccepted).not.toHaveBeenCalled();
  });

  it("授权调用的非法参数也消费预算并留下失败动作，但不能指定状态或伪造身份", async () => {
    const f = setup(); f.need();
    const invalid = [{ status: "supported" }, { actorSessionId: "other" }, { conclusionSummary: 42 },
      { requestedStopReason: "anything" }, { requestedStopReason: { toString: () => "complete" } }];
    for (const params of invalid) expect((await f.finish("complete", params)).details)
      .toMatchObject({ errorCode: "KNOWLEDGE_INVALID_ARGUMENT" });
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(invalid.length);
    expect(f.research.listActions(f.run.id).every(action => action.status === "failed")).toBe(true);
    expect(f.onFinishAccepted).not.toHaveBeenCalled();
  });

  it("结束摘要不落库、不回显，也不传给后续宿主回调", async () => {
    const f = setup(); const need = f.need(); f.support(need.id);
    const secret = "禁止落盘的原始结论及模型思考MARKER";
    const result = await f.finish("complete", { conclusionSummary: secret });
    expect(payload(result).accepted).toBe(true);
    expect(JSON.stringify(result)).not.toContain(secret);
    expect(JSON.stringify(f.onFinishAccepted.mock.calls)).not.toContain(secret);
    const action = f.research.listActions(f.run.id)[0];
    expect(action.requestSummary).toEqual({});
    expect(action.responseSummary).toEqual({ count: 1, status: "accepted" });
    expect(JSON.stringify(f.store.db.prepare("SELECT * FROM knowledge_research_actions").all())).not.toContain(secret);
    expect(JSON.stringify(f.research.requireRun(f.run.id))).not.toContain(secret);
  });

  it("已取消的调用不创建动作、不触发结束回调", async () => {
    const f = setup(); f.need(); const controller = new AbortController(); controller.abort();
    await expect(f.finish("complete", {}, controller.signal)).rejects.toThrow();
    expect(f.research.listActions(f.run.id)).toEqual([]);
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(0);
    expect(f.onFinishAccepted).not.toHaveBeenCalled();
  });
});
