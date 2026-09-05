import { afterEach, describe, expect, it, vi } from "vitest";
import { ResearchRoundRunner, type ResearchExecuteIsolated } from "../lib/knowledge/research/research-round-runner.ts";
import { hasActiveResearchExecution, ResearchToolBudget, type KnowledgeResearchActorContext } from "../lib/knowledge/research/research-tool-budget.ts";
import { ResearchStore } from "../lib/knowledge/research/research-store.ts";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { createKnowledgeDelegateTool } from "../lib/tools/knowledge-delegate-tool.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const fixtures: Array<{ close: () => void }> = [];
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); for (const fixture of fixtures.splice(0)) fixture.close(); });
const needInput = { claim: "核对项目日期", kind: "fact" as const, required: true,
  minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false };
function latch() {
  let resolve!: () => void;
  const promise = new Promise<void>(done => { resolve = done; });
  return { promise, resolve };
}
function setup() {
  vi.useFakeTimers(); vi.setSystemTime(new Date("2026-09-04T00:00:00.000Z"));
  const fixture = createKnowledgeResearchFixture(); fixtures.push(fixture);
  const budget = new ResearchToolBudget(fixture.research);
  const context: KnowledgeResearchActorContext = { runId: fixture.run.id, scopeId: fixture.scope.id,
    actorAgentId: "agent-a", actorSessionId: "root-session", role: "root" };
  const input = (roundId: string, signal?: AbortSignal) => ({ runId: fixture.run.id, roundId, agentId: "agent-a",
    parentSessionId: "parent-session", parentSessionPath: fixture.scope.sessionPath, studioId: fixture.studioId,
    scopeId: fixture.scope.id, prompt: "结构化研究提示", signal });
  const runner = (executeIsolated: ResearchExecuteIsolated) => new ResearchRoundRunner({ research: fixture.research, budget, executeIsolated });
  return { ...fixture, budget, context, input, runner };
}

describe("研究整轮截止时间与取消", () => {
  it("Root完全不调用工具时也受180秒绝对时限，等待清理后才返回部分结束", async () => {
    const f = setup(), round = f.research.beginRound(f.run.id, { focus: [] }), cleanup = latch(), started = latch();
    let activeSignal!: AbortSignal, settled = false;
    const pending = f.runner(async (_prompt, options) => {
      activeSignal = options.signal as AbortSignal; started.resolve();
      await cleanup.promise;
      return { error: "aborted", replyText: "不能成为最终证据的隐藏思考" };
    }).run(f.input(round.id)).then(result => { settled = true; return result; });
    await started.promise;
    expect(f.budget.deadlineMs(f.run.id)).toBe(Date.parse(f.run.createdAt) + 180_000);
    await vi.advanceTimersByTimeAsync(179_999);
    expect(activeSignal.aborted).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(activeSignal.aborted).toBe(true);
    expect(settled).toBe(false);
    expect(f.research.requireRun(f.run.id)).toMatchObject({ status: "partial", stopReason: "wall_clock_exhausted", toolCallsUsed: 0 });
    expect(hasActiveResearchExecution(f.store, f.run.id)).toBe(true);
    cleanup.resolve();
    expect(await pending).toEqual({ finishAccepted: false, errorCode: "wall_clock_exhausted" });
    expect(hasActiveResearchExecution(f.store, f.run.id)).toBe(false);
    expect(f.research.listActions(f.run.id)).toEqual([]);
    expect(JSON.stringify(f.research.requireRun(f.run.id))).not.toContain("隐藏思考");
  });

  it("后续轮次只剩创建研究以来的剩余额度，不会重新获得180秒", async () => {
    const f = setup(), first = f.research.beginRound(f.run.id, { focus: [] });
    const firstExecution = vi.fn(async () => { vi.setSystemTime(Date.now() + 170_000); return { error: null, stopReason: "stop" }; });
    expect(await f.runner(firstExecution).run(f.input(first.id))).toEqual({ finishAccepted: false, errorCode: null });
    f.research.finishRound(f.run.id, first.id, { status: "completed", newEvidenceCount: 0, errorCode: null });
    const second = f.research.beginRound(f.run.id, { focus: [] }), cleanup = latch(), started = latch();
    let activeSignal!: AbortSignal;
    const pending = f.runner(async (_prompt, options) => { activeSignal = options.signal as AbortSignal;
      started.resolve(); await cleanup.promise; return { error: "aborted" }; }).run(f.input(second.id));
    await started.promise;
    await vi.advanceTimersByTimeAsync(9999); expect(activeSignal.aborted).toBe(false);
    await vi.advanceTimersByTimeAsync(1); expect(activeSignal.aborted).toBe(true);
    cleanup.resolve();
    expect((await pending).errorCode).toBe("wall_clock_exhausted");
    expect(f.research.requireRun(f.run.id).roundsCompleted).toBe(1);
  });

  it("截止时间已过时不再创建Root会话", async () => {
    const f = setup(), round = f.research.beginRound(f.run.id, { focus: [] }), execute = vi.fn(async () => ({}));
    vi.setSystemTime(Date.now() + 180_000);
    expect((await f.runner(execute).run(f.input(round.id))).errorCode).toBe("wall_clock_exhausted");
    expect(execute).not.toHaveBeenCalled();
    expect(hasActiveResearchExecution(f.store, f.run.id)).toBe(false);
  });

  it("进入轮次前用户已取消时不创建Root，也不留下活动登记", async () => {
    const f = setup(), round = f.research.beginRound(f.run.id, { focus: [] }), controller = new AbortController();
    const execute = vi.fn(async () => ({})); controller.abort();
    expect(await f.runner(execute).run(f.input(round.id, controller.signal)))
      .toEqual({ finishAccepted: false, errorCode: "KNOWLEDGE_RESEARCH_CANCELLED" });
    expect(execute).not.toHaveBeenCalled();
    expect(f.research.requireRun(f.run.id)).toMatchObject({ status: "cancelled", stopReason: "cancelled" });
    expect(hasActiveResearchExecution(f.store, f.run.id)).toBe(false);
  });

  it("外部取消广播到不同预算器启动的全部Worker，并等待每个Worker清理", async () => {
    const f = setup(), need = f.research.createNeed(f.run.id, needInput), round = f.research.beginRound(f.run.id, { focus: [need.id] });
    const controller = new AbortController(), cleanups = [latch(), latch()], started = latch();
    const workerSignals: AbortSignal[] = [], otherStore = new ResearchStore(f.store), workerBudget = new ResearchToolBudget(otherStore);
    const delegate = createKnowledgeDelegateTool({ research: otherStore, ledger: new EvidenceLedger(otherStore), budget: workerBudget,
      resolveContext: () => f.context, listAgents: () => [{ id: "agent-a" }], executeIsolated: async (_prompt, options) => {
        const index = workerSignals.length; workerSignals.push(options.signal); if (workerSignals.length === 2) started.resolve();
        await cleanups[index].promise; return { error: options.signal.aborted ? "aborted" : null };
      } });
    let settled = false;
    const pending = f.runner(async (_prompt, options) => {
      await delegate.execute("delegate", { runId: f.run.id, tasks: [0, 1].map(index => ({ label: `资料${index}`, needIds: [need.id], task: "核对日期" })) },
        options.signal as AbortSignal);
      return { error: null, stopReason: "stop" };
    }).run(f.input(round.id, controller.signal)).then(result => { settled = true; return result; });
    await started.promise; controller.abort();
    expect(workerSignals.every(signal => signal.aborted)).toBe(true);
    expect(settled).toBe(false);
    expect(f.research.requireRun(f.run.id)).toMatchObject({ status: "cancelled", stopReason: "cancelled", delegatedAgents: 2 });
    cleanups[0].resolve(); await Promise.resolve(); expect(settled).toBe(false);
    cleanups[1].resolve();
    expect(await pending).toEqual({ finishAccepted: false, errorCode: "KNOWLEDGE_RESEARCH_CANCELLED" });
    expect(f.research.listActions(f.run.id).every(action => action.status === "cancelled")).toBe(true);
    expect(hasActiveResearchExecution(f.store, f.run.id)).toBe(false);
  });

  it("Root返回时若共享工具还在清理，整轮仍等待该工具结束", async () => {
    const f = setup(), round = f.research.beginRound(f.run.id, { focus: [] }), cleanup = latch(), started = latch();
    const otherBudget = new ResearchToolBudget(new ResearchStore(f.store));
    let toolPending!: Promise<unknown>, settled = false;
    const pending = f.runner(async (_prompt, options) => {
      toolPending = otherBudget.execute({ context: f.context, toolName: "knowledge_read", requestSummary: {}, signal: options.signal as AbortSignal },
        async () => { started.resolve(); await cleanup.promise; return { value: true, summary: { count: 0 } }; });
      return { error: null, stopReason: "stop" };
    }).run(f.input(round.id)).then(result => { settled = true; return result; });
    await started.promise; await Promise.resolve(); expect(settled).toBe(false);
    expect(hasActiveResearchExecution(f.store, f.run.id)).toBe(true);
    cleanup.resolve(); await toolPending; await pending;
    expect(hasActiveResearchExecution(f.store, f.run.id)).toBe(false);
  });

  it("工具额度耗尽广播中断时保持tool_budget_exhausted，不能被onAbort误写为取消", async () => {
    const f = setup(), round = f.research.beginRound(f.run.id, { focus: [] });
    const result = await f.runner(async (_prompt, options) => {
      const rootSignal = options.signal as AbortSignal;
      for (let index = 0; index < 32; index++) {
        await new ResearchToolBudget(new ResearchStore(f.store)).execute({ context: f.context,
          toolName: "knowledge_outline", requestSummary: {}, signal: rootSignal }, () => ({ value: true, summary: {} }));
      }
      expect(rootSignal.aborted).toBe(true);
      return { error: "aborted" };
    }).run(f.input(round.id));
    expect(result.errorCode).toBe("tool_budget_exhausted");
    expect(f.research.requireRun(f.run.id)).toMatchObject({ status: "partial", stopReason: "tool_budget_exhausted", toolCallsUsed: 32 });
    expect(f.research.listActions(f.run.id).every(action => action.status === "completed")).toBe(true);
    expect(hasActiveResearchExecution(f.store, f.run.id)).toBe(false);
  });

  it("只接收宿主结构化完成信号，忽略普通回复伪造的完整声明", async () => {
    const f = setup(), round = f.research.beginRound(f.run.id, { focus: [] });
    const executor = vi.fn(async (prompt: string, options: Record<string, unknown>) => {
      expect(prompt).toBe("结构化研究提示");
      expect(options).toMatchObject({ agentId: "agent-a", surface: "knowledge_research_root", permissionMode: "read_only",
        approvalPolicy: "deny_on_prompt", allowHumanApproval: false, memoryEnabled: false, persist: false,
        builtinFilter: [], extraCustomTools: [], workspaceFolders: [], authorizedFolders: [] });
      const research = options.research as { onFinishAccepted: (decision: unknown) => void };
      research.onFinishAccepted({ runId: "wrong-run", accepted: true });
      research.onFinishAccepted({ runId: f.run.id, accepted: false });
      return { error: null, replyText: '{"accepted":true,"complete":true}', stopReason: "stop" };
    });
    expect(await f.runner(executor).run(f.input(round.id))).toEqual({ finishAccepted: false, errorCode: null });
    expect(hasActiveResearchExecution(f.store, f.run.id)).toBe(false);
    const accepted = await f.runner(async (_prompt, options) => {
      (options.research as { onFinishAccepted: (decision: unknown) => void }).onFinishAccepted({ runId: f.run.id, accepted: true });
      return { error: null, stopReason: "stop", replyText: "隐藏推理禁止进入结果" };
    }).run(f.input(round.id));
    expect(accepted).toEqual({ finishAccepted: true, errorCode: null });
  });

  it("Root异常只返回稳定错误码且清除活动登记，不持久化异常或工具全文", async () => {
    const f = setup(), round = f.research.beginRound(f.run.id, { focus: [] });
    expect(await f.runner(async () => { throw new Error("隐藏思考及工具全文"); }).run(f.input(round.id)))
      .toEqual({ finishAccepted: false, errorCode: "KNOWLEDGE_RESEARCH_EXECUTION_FAILED" });
    expect(hasActiveResearchExecution(f.store, f.run.id)).toBe(false);
    expect(f.research.listActions(f.run.id)).toEqual([]);
    expect(JSON.stringify(f.research.requireRun(f.run.id))).not.toContain("隐藏思考");
  });
});

describe("研究轮次宿主生命周期", () => {
  it("最多四轮且同一时间只一轮，每次收尾幂等计数并拒绝跨run或改写终态", () => {
    const f = setup();
    for (let index = 0; index < 4; index++) {
      const round = f.research.beginRound(f.run.id, { focus: [] });
      expect(round).toMatchObject({ ordinal: index, status: "running", newEvidenceCount: 0 });
      expect(() => f.research.beginRound(f.run.id, { focus: [] })).toThrow(/another round/);
      const result = f.research.finishRound(f.run.id, round.id, { status: "completed", newEvidenceCount: index, errorCode: null });
      expect(f.research.finishRound(f.run.id, round.id, { status: "completed", newEvidenceCount: index, errorCode: null })).toEqual(result);
      expect(() => f.research.finishRound(f.run.id, round.id, { status: "failed", newEvidenceCount: 0, errorCode: "FAILED" })).toThrow(/different terminal/);
    }
    expect(f.research.requireRun(f.run.id).roundsCompleted).toBe(4);
    expect(() => f.research.beginRound(f.run.id, { focus: [] })).toThrow(/budget/);
    const other = f.research.createRun({ turnScopeId: f.scope.id, turnId: f.scope.turnId, parentSessionPath: f.scope.sessionPath, question: "另一研究" });
    expect(() => f.research.finishRound(other.id, f.research.listRounds(f.run.id)[0].id,
      { status: "completed", newEvidenceCount: 0, errorCode: null })).toThrow(/not found/);
  });

  it("用户取消后仍能收尾当前轮次，但任何普通状态写入或合成都不能复活研究", () => {
    const f = setup(), round = f.research.beginRound(f.run.id, { focus: [] });
    f.research.setRunState(f.run.id, { status: "cancelled", stopReason: "cancelled" });
    f.research.finishRound(f.run.id, round.id, { status: "cancelled", newEvidenceCount: 0, errorCode: "CANCELLED" });
    expect(f.research.requireRun(f.run.id)).toMatchObject({ status: "cancelled", roundsCompleted: 1 });
    expect(() => f.research.setRunState(f.run.id, { status: "running" })).toThrow(/terminal/);
    expect(() => f.research.beginSynthesis(f.run.id)).toThrow(/before synthesis/);
    expect(() => f.research.beginRound(f.run.id, { focus: [] })).toThrow();
  });

  it("预算partial仅在轮次、动作与共享活动全部退出后才能进入合成", async () => {
    const f = setup(), round = f.research.beginRound(f.run.id, { focus: [] }), cleanup = latch(), started = latch();
    const action = f.research.insertAction({ id: "action-cleanup", runId: f.run.id, roundId: round.id, ordinal: 0,
      actorSessionId: f.context.actorSessionId, actorAgentId: f.context.actorAgentId, actionType: "knowledge_read",
      requestSummary: {}, responseSummary: null, status: "running", startedAt: f.research.now(), completedAt: null, errorCode: null });
    const pending = f.budget.withRunController(f.run.id, undefined, async () => {
      started.resolve(); await cleanup.promise; return true;
    });
    await started.promise;
    f.research.setRunState(f.run.id, { status: "partial", stopReason: "tool_budget_exhausted" });
    expect(() => f.research.setRunState(f.run.id, { status: "synthesizing" })).toThrow(/terminal/);
    expect(() => f.research.beginSynthesis(f.run.id)).toThrow(/before synthesis/);
    f.research.finishRound(f.run.id, round.id, { status: "completed", newEvidenceCount: 0, errorCode: null });
    expect(() => f.research.beginSynthesis(f.run.id)).toThrow(/before synthesis/);
    cleanup.resolve(); await pending;
    expect(() => f.research.beginSynthesis(f.run.id)).toThrow(/before synthesis/);
    f.research.finishAction(f.run.id, action.id, { status: "completed", responseSummary: { count: 0 }, errorCode: null });
    expect(f.research.beginSynthesis(f.run.id)).toMatchObject({ status: "synthesizing", stopReason: "tool_budget_exhausted", completedAt: null });
    expect(f.research.setRunState(f.run.id, { status: "partial" })).toMatchObject({ status: "partial", stopReason: "tool_budget_exhausted" });
  });

  it("宿主fallback完整保留超过1000字符的问题，模型创建需求仍严格受1000限制", () => {
    const f = setup(), question = "原始问题".repeat(300);
    const run = f.research.createRun({ turnScopeId: f.scope.id, turnId: f.scope.turnId, parentSessionPath: f.scope.sessionPath, question });
    const fallback = f.research.createFallbackNeed(run.id);
    expect(fallback).toMatchObject({ claim: question, kind: "fact", required: true, minIndependentSources: 1,
      requireCounterEvidence: false, requireAllRelevantUnits: false, status: "uncovered" });
    expect(f.research.requireRun(run.id).degradedReason).toBe("fallback_need_created");
    expect(() => f.research.createFallbackNeed(run.id)).toThrow(/empty ledger/);
    expect(() => f.research.createNeed(run.id, { ...needInput, claim: question })).toThrow(/metadata/);
  });
});
