import { afterEach, describe, expect, it } from "vitest";
import { ResearchToolBudget, type KnowledgeResearchActorContext } from "../lib/knowledge/research/research-tool-budget.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const fixtures: ReturnType<typeof createKnowledgeResearchFixture>[] = [];
afterEach(() => { for (const f of fixtures.splice(0)) f.close(); });
function setup() {
  const f = createKnowledgeResearchFixture(); fixtures.push(f);
  let now = Date.parse(f.run.createdAt);
  const budget = new ResearchToolBudget(f.research, { nowMs: () => now });
  const context: KnowledgeResearchActorContext = { runId: f.run.id, scopeId: f.scope.id,
    actorSessionId: "root-session", actorAgentId: "agent-a", role: "root" };
  const execute = (toolName = "knowledge_outline", body = () => ({ value: true, summary: { count: 1 } })) =>
    budget.execute({ context, toolName, requestSummary: {} }, body);
  return { ...f, budget, context, execute, advance: (ms: number) => { now += ms; } };
}
function latch() {
  let resolve!: () => void;
  const promise = new Promise<void>(done => { resolve = done; });
  return { promise, resolve };
}

describe("研究工具共用宿主预算", () => {
  it("不同预算器仍共享32次总额，到点部分结束并拒绝第33次", async () => {
    const f = setup();
    for (let index = 0; index < 32; index++) {
      const budget = new ResearchToolBudget(f.research);
      await budget.execute({ context: f.context, toolName: "knowledge_outline", requestSummary: {} },
        () => ({ value: index, summary: { count: 1 } }));
    }
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 32, status: "partial", stopReason: "tool_budget_exhausted" });
    await expect(f.execute()).rejects.toThrow(/budget/);
    expect(f.research.listActions(f.run.id)).toHaveLength(32);
    expect(f.research.listActions(f.run.id).every(action => action.status === "completed")).toBe(true);
  });

  it("并发请求原子预占总额，不会因多工作会话同时开始越过32次", async () => {
    const f = setup(); let entered = 0;
    const results = await Promise.allSettled(Array.from({ length: 40 }, () => f.budget.execute({
      context: f.context, toolName: "knowledge_outline", requestSummary: {},
    }, async () => { entered += 1; await Promise.resolve(); return { value: true, summary: {} }; })));
    expect(results).toHaveLength(40);
    expect(entered).toBe(32);
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(32);
    expect(f.research.listActions(f.run.id)).toHaveLength(32);
    expect(f.research.listActions(f.run.id).some(action => action.status === "running")).toBe(false);
  });

  it("每轮搜索最多8次且阅读最多12次，拒绝调用也有失败动作记录", async () => {
    const search = setup();
    for (let index = 0; index < 8; index++) await search.execute("knowledge_search");
    let searched = false;
    await expect(search.execute("knowledge_search", () => { searched = true; return { value: true, summary: { count: 0 } }; })).rejects.toThrow();
    expect(searched).toBe(false);
    expect(search.research.requireRun(search.run.id)).toMatchObject({ status: "planning", toolCallsUsed: 9, searchCalls: 9 });
    expect(search.research.listActions(search.run.id).at(-1)).toMatchObject({ status: "failed", errorCode: "round_search_limit" });
    const read = setup();
    for (let index = 0; index < 12; index++) await read.execute("knowledge_read");
    await expect(read.execute("knowledge_read")).rejects.toThrow();
    expect(read.research.listActions(read.run.id).at(-1)).toMatchObject({ status: "failed", errorCode: "round_read_limit" });
  });

  it("新一轮重置本轮次数，但不重置整次研究的已用时间和调用数", async () => {
    const f = setup();
    for (let index = 0; index < 8; index++) await f.execute("knowledge_search");
    f.store.db.prepare(`INSERT INTO knowledge_research_rounds(id,run_id,ordinal,focus_json,status,started_at)
      VALUES(?,?,0,'[]','running',?)`).run("second-round", f.run.id, f.research.now());
    await f.execute("knowledge_search");
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 9, searchCalls: 9 });
    expect(f.research.listActions(f.run.id).at(-1)?.roundId).toBe("second-round");
    f.advance(180_000);
    await expect(f.execute()).rejects.toThrow(/budget/);
    expect(f.research.requireRun(f.run.id)).toMatchObject({ status: "partial", stopReason: "wall_clock_exhausted", toolCallsUsed: 9 });
  });

  it("完整性阅读与普通阅读共享每轮12次额度和真实阅读计数", async () => {
    const f = setup();
    for (let index = 0; index < 12; index++) await f.execute(index % 2 ? "knowledge_coverage_read" : "knowledge_read");
    let entered = false;
    await expect(f.execute("knowledge_coverage_read", () => { entered = true; return { value: true, summary: { count: 1 } }; })).rejects.toThrow();
    expect(entered).toBe(false);
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 13, readCalls: 13 });
    expect(f.research.listActions(f.run.id).at(-1)).toMatchObject({ actionType: "knowledge_coverage_read", status: "failed", errorCode: "round_read_limit" });
  });

  it("调用结束才越过总时限也必须拒绝结果，不能把超时返回当成功", async () => {
    const f = setup();
    await expect(f.execute("knowledge_outline", () => { f.advance(180_000); return { value: true, summary: { count: 1 } }; })).rejects.toThrow();
    expect(f.research.requireRun(f.run.id)).toMatchObject({ status: "partial", stopReason: "wall_clock_exhausted" });
    expect(f.research.listActions(f.run.id)[0].status).toBe("cancelled");
  });

  it("错误照常计数，只保存错误码而不把异常正文写进动作摘要", async () => {
    const f = setup();
    await expect(f.execute("knowledge_grep", () => { throw new Error("禁止落盘的用户正文"); })).rejects.toThrow();
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 1, grepCalls: 1 });
    const actions = f.research.listActions(f.run.id);
    expect(actions[0]).toMatchObject({ status: "failed", errorCode: "RESEARCH_TOOL_FAILED", responseSummary: null });
    expect(JSON.stringify(actions)).not.toContain("禁止落盘的用户正文");
  });

  it("取消同时传给全部调用，真实清理结束前不返回或释放名额", async () => {
    const f = setup(); const cleanup = latch(); let aborted = 0; let settled = false;
    const running = f.budget.withWorkerSlots(f.run.id, 2, async () => {
      const workers = [0, 1].map(() => f.budget.execute({ context: f.context, toolName: "knowledge_read", requestSummary: {} }, async signal => {
        signal.addEventListener("abort", () => { aborted++; }, { once: true });
        await cleanup.promise;
        return { value: true, summary: {} };
      }));
      await Promise.allSettled(workers);
    }).finally(() => { settled = true; });
    await Promise.resolve();
    f.budget.cancel(f.run.id);
    await Promise.resolve();
    expect(aborted).toBe(2); expect(settled).toBe(false);
    cleanup.resolve(); await running;
    expect(settled).toBe(true);
    expect(f.research.requireRun(f.run.id)).toMatchObject({ status: "cancelled", delegatedAgents: 2 });
    expect(f.research.listActions(f.run.id).every(action => action.status === "cancelled")).toBe(true);
  });

  it("登记委派计数写库失败必须退还名额，解除故障后可立即重试", async () => {
    const f = setup();
    f.store.db.exec(`CREATE TEMP TRIGGER fail_delegation BEFORE UPDATE OF delegated_agents
      ON knowledge_research_runs BEGIN SELECT RAISE(ABORT, 'delegation-write-failed'); END;`);
    await expect(f.budget.withWorkerSlots(f.run.id, 4, async () => true)).rejects.toThrow("delegation-write-failed");
    expect(f.research.requireRun(f.run.id).delegatedAgents).toBe(0);
    f.store.db.exec("DROP TRIGGER fail_delegation");
    await expect(f.budget.withWorkerSlots(f.run.id, 4, async () => true)).resolves.toBe(true);
    expect(f.research.requireRun(f.run.id).delegatedAgents).toBe(4);
  });

  it("跨run身份或越冻结范围在执行前拒绝，不污染其他运行计数", async () => {
    const f = setup();
    await expect(f.budget.execute({ context: { ...f.context, scopeId: "outside" },
      toolName: "knowledge_outline", requestSummary: {} }, () => ({ value: true, summary: {} }))).rejects.toThrow();
    await expect(f.budget.execute({ context: { ...f.context, role: "worker", allowedNeedIds: [] },
      toolName: "knowledge_outline", requestSummary: {} }, () => ({ value: true, summary: {} }))).rejects.toThrow();
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(0);
    expect(f.research.listActions(f.run.id)).toEqual([]);
  });
});
