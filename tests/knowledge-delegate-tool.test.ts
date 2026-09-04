import { afterEach, describe, expect, it, vi } from "vitest";
import { createKnowledgeDelegateTool, type KnowledgeResearchWorkerOptions } from "../lib/tools/knowledge-delegate-tool.ts";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { ResearchToolBudget, type KnowledgeResearchActorContext } from "../lib/knowledge/research/research-tool-budget.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const fixtures: ReturnType<typeof createKnowledgeResearchFixture>[] = [];
afterEach(() => {
  vi.restoreAllMocks();
  for (const fixture of fixtures.splice(0)) fixture.close();
});

function setup() {
  const data = createKnowledgeResearchFixture();
  fixtures.push(data);
  const need = data.research.createNeed(data.run.id, {
    claim: "确定交付日期", kind: "fact", required: true, minIndependentSources: 1,
    requireCounterEvidence: false, requireAllRelevantUnits: false,
  });
  const context: KnowledgeResearchActorContext = { runId: data.run.id, scopeId: data.scope.id,
    actorSessionId: "parent-session", actorAgentId: "agent-current", role: "root" };
  const executeIsolated = vi.fn(async (_prompt: string, _options: KnowledgeResearchWorkerOptions): Promise<unknown> => ({
    replyText: "不应该进入父工具结果的模型原文", error: null, stopReason: "stop",
  }));
  const ledger = new EvidenceLedger(data.research);
  const budget = new ResearchToolBudget(data.research);
  const deps = { research: data.research, ledger, budget, resolveContext: () => context,
    listAgents: () => [{ id: "agent-current" }, { id: "agent-other", status: "active" }], executeIsolated };
  const task = (label = "核对日期", agentId?: string) => ({ label, needIds: [need.id], task: "读取资料并核对日期",
    ...(agentId === undefined ? {} : { agentId }) });
  return { ...data, need, context, executeIsolated, ledger, budget, deps, task, tool: createKnowledgeDelegateTool(deps) };
}

function parse(result: { isError?: true; content: Array<{ text: string }> }) {
  expect(result.isError, result.content[0].text).not.toBe(true);
  return JSON.parse(result.content[0].text);
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe("knowledge_delegate 有界同步委派", () => {
  it("复用当前 Agent 并传递真实只读隔离选项、范围和当前需求状态", async () => {
    const data = setup();
    const result = parse(await data.tool.execute("delegate", { runId: data.run.id, tasks: [data.task()] }));
    expect(result).toMatchObject({ runId: data.run.id, status: "completed", tasks: [{ agentId: "agent-current", status: "completed", needIds: [data.need.id] }] });
    const [prompt, options] = data.executeIsolated.mock.calls[0];
    const assignment = JSON.parse(prompt.slice(prompt.indexOf("\n") + 1));
    expect(assignment).toMatchObject({ question: data.run.question, scopeId: data.scope.id, runId: data.run.id,
      needIds: [data.need.id], needs: [{ id: data.need.id, claim: data.need.claim, status: "uncovered" }] });
    expect(prompt).toContain("不得修改资料");
    expect(prompt).toContain("再次委派任务或调用研究完成工具");
    expect(options).toMatchObject({ agentId: "agent-current", parentSessionPath: data.scope.sessionPath,
      permissionMode: "read_only", approvalPolicy: "deny_on_prompt", allowHumanApproval: false, subagentContext: true,
      surface: "knowledge_research_worker", builtinFilter: [],
      researchContext: { runId: data.run.id, scopeId: data.scope.id, role: "worker", actorSessionId: null,
        actorAgentId: "agent-current", allowedNeedIds: [data.need.id] },
    });
    expect(options.toolFilter).toEqual(["knowledge_outline", "knowledge_search", "knowledge_read", "knowledge_grep", "knowledge_research_update"]);
    expect(options).not.toHaveProperty("persist");
    expect(options).not.toHaveProperty("resumeSessionPath");
    expect(options.signal).toBeInstanceOf(AbortSignal);
    expect(JSON.stringify(result)).not.toContain("不应该进入父工具结果");
    expect(data.research.requireRun(data.run.id)).toMatchObject({ delegatedAgents: 1, toolCallsUsed: 1 });
    const actions = data.research.listActions(data.run.id);
    expect(actions).toHaveLength(1);
    expect(actions[0]).toMatchObject({ actionType: "knowledge_delegate", status: "completed",
      responseSummary: { count: 1, status: "completed" } });
    expect(JSON.stringify(actions)).not.toContain("不应该进入父工具结果");
    expect(JSON.stringify(actions)).not.toContain("读取资料并核对日期");
  });

  it("多个隔离会话实际并行启动，必须全部结束才返回", async () => {
    const data = setup();
    const first = deferred<unknown>(), second = deferred<unknown>();
    data.executeIsolated.mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise);
    let settled = false;
    const pending = data.tool.execute("parallel", { runId: data.run.id, tasks: [data.task("甲"), data.task("乙", "agent-other")] })
      .finally(() => { settled = true; });
    await vi.waitFor(() => expect(data.executeIsolated).toHaveBeenCalledTimes(2));
    expect(settled).toBe(false);
    first.resolve({ error: null, stopReason: "stop" });
    await Promise.resolve();
    expect(settled).toBe(false);
    second.resolve({ error: null, stopReason: "stop" });
    const result = parse(await pending);
    expect(result.tasks.map((task: { agentId: string }) => task.agentId)).toEqual(["agent-current", "agent-other"]);
    expect(data.research.requireRun(data.run.id).delegatedAgents).toBe(2);
  });

  it("一个会话失败仍等待其它会话结束，返回固定错误码并剥离原始回答和异常文本", async () => {
    const data = setup();
    const slow = deferred<unknown>();
    data.executeIsolated.mockImplementationOnce(async () => { throw new Error("私密模型原文以及本地路径"); })
      .mockImplementationOnce(() => slow.promise);
    let settled = false;
    const pending = data.tool.execute("mixed", { runId: data.run.id, tasks: [data.task("失败任务"), data.task("其它任务")] })
      .finally(() => { settled = true; });
    await vi.waitFor(() => expect(data.executeIsolated).toHaveBeenCalledTimes(2));
    expect(settled).toBe(false);
    slow.resolve({ replyText: "其它工作会话原始回答", error: null, stopReason: "stop" });
    const result = parse(await pending);
    expect(result.status).toBe("partial");
    expect(result.tasks[0]).toMatchObject({ status: "failed", errorCode: "KNOWLEDGE_RESEARCH_WORKER_FAILED" });
    expect(result.tasks[1].status).toBe("completed");
    expect(JSON.stringify(result)).not.toMatch(/私密模型原文|本地路径|其它工作会话原始回答/);
    expect(JSON.stringify(data.research.listActions(data.run.id))).not.toMatch(/私密模型原文|本地路径|其它工作会话原始回答/);
  });

  it("不同工具和预算实例共享同一运行的并发槽，拒绝超额且完成后释放", async () => {
    const data = setup();
    const first = deferred<unknown>(), second = deferred<unknown>();
    data.executeIsolated.mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise);
    const running = data.tool.execute("first-two", { runId: data.run.id, tasks: [data.task("一"), data.task("二")] });
    await vi.waitFor(() => expect(data.executeIsolated).toHaveBeenCalledTimes(2));
    const other = createKnowledgeDelegateTool({ ...data.deps, budget: new ResearchToolBudget(data.research) });
    const rejected = await other.execute("three-more", { runId: data.run.id,
      tasks: [data.task("三"), data.task("四"), data.task("五")] });
    expect(rejected.isError).toBe(true);
    expect(data.executeIsolated).toHaveBeenCalledTimes(2);
    expect(data.research.requireRun(data.run.id).delegatedAgents).toBe(2);
    first.resolve({ stopReason: "stop" }); second.resolve({ stopReason: "stop" });
    parse(await running);
    const accepted = parse(await other.execute("slots-released", { runId: data.run.id,
      tasks: [data.task("三"), data.task("四"), data.task("五")] }));
    expect(accepted.status).toBe("completed");
    expect(data.executeIsolated).toHaveBeenCalledTimes(5);
    expect(data.research.requireRun(data.run.id).delegatedAgents).toBe(5);
  });

  it("取消传给每个会话，并在会话清理完成前继续等待", async () => {
    const data = setup();
    const first = deferred<unknown>(), second = deferred<unknown>();
    const signals: AbortSignal[] = [];
    data.executeIsolated.mockImplementationOnce((_prompt, options) => { signals.push(options.signal); return first.promise; })
      .mockImplementationOnce((_prompt, options) => { signals.push(options.signal); return second.promise; });
    const abort = new AbortController();
    let settled = false;
    const pending = data.tool.execute("cancel", { runId: data.run.id, tasks: [data.task("甲"), data.task("乙")] }, abort.signal)
      .finally(() => { settled = true; });
    await vi.waitFor(() => expect(signals).toHaveLength(2));
    abort.abort();
    expect(signals.every(signal => signal.aborted)).toBe(true);
    await Promise.resolve();
    expect(settled).toBe(false);
    first.resolve({ error: "aborted" });
    await Promise.resolve();
    expect(settled).toBe(false);
    second.resolve({ error: "aborted" });
    const result = await pending;
    expect(result.isError).toBe(true);
    expect(data.research.listActions(data.run.id)[0].status).toBe("cancelled");
    expect(data.research.requireRun(data.run.id).status).toBe("cancelled");
    const later = await data.tool.execute("after-cancel", { runId: data.run.id, tasks: [data.task()] });
    expect(later.isError).toBe(true);
    expect(data.executeIsolated).toHaveBeenCalledTimes(2);
  });

  it("整批任务先校验，空任务、超四项、缺需求、非法成员均不启动工作会话且记入调用次数", async () => {
    const data = setup();
    const invalidTasks = [
      [], Array.from({ length: 5 }, () => data.task()),
      [{ ...data.task(), needIds: [] }], [{ ...data.task(), label: " " }], [{ ...data.task(), task: "" }],
      [data.task(), data.task("无效成员", "missing-agent")],
      [{ ...data.task(), needIds: ["missing-need"] }],
      [{ ...data.task(), permissionMode: "operate" }],
    ];
    for (const tasks of invalidTasks) {
      const result = await data.tool.execute("invalid", { runId: data.run.id, tasks });
      expect(result.isError).toBe(true);
    }
    expect(data.executeIsolated).not.toHaveBeenCalled();
    expect(data.research.requireRun(data.run.id)).toMatchObject({ toolCallsUsed: invalidTasks.length, delegatedAgents: 0 });
    expect(data.research.listActions(data.run.id).every(action => action.status === "failed")).toBe(true);
  });

  it("显式停用或归档成员不能执行，当前成员不存在也不能默认绕过名单", async () => {
    const data = setup();
    const tool = createKnowledgeDelegateTool({ ...data.deps, listAgents: () => [
      { id: "inactive", status: "inactive" }, { id: "archived", status: "active", archived: true },
    ] });
    for (const task of [data.task(), data.task("停用", "inactive"), data.task("归档", "archived")]) {
      expect((await tool.execute("invalid-agent", { runId: data.run.id, tasks: [task] })).isError).toBe(true);
    }
    expect(data.executeIsolated).not.toHaveBeenCalled();
  });

  it("顶层额外字段和非普通对象拒绝授权或状态注入，并计入失败动作", async () => {
    const data = setup();
    const invalidParams = [
      { runId: data.run.id, tasks: [data.task()], permissionMode: "operate" },
      { runId: data.run.id, tasks: [data.task()], status: "completed" },
      { runId: data.run.id, tasks: [data.task()], actorAgentId: "agent-other" },
      { runId: data.run.id, tasks: [data.task()], role: "root" },
      Object.assign([], { runId: data.run.id, tasks: [data.task()] }),
      Object.assign(Object.create({ permissionMode: "operate" }), { runId: data.run.id, tasks: [data.task()] }),
    ];
    for (const params of invalidParams) {
      const result = await data.tool.execute("top-level-injection", params);
      expect(result.isError).toBe(true);
      expect(result.details).toMatchObject({ errorCode: "KNOWLEDGE_INVALID_ARGUMENT" });
    }
    expect(data.executeIsolated).not.toHaveBeenCalled();
    expect(data.research.requireRun(data.run.id)).toMatchObject({ status: "planning", toolCallsUsed: invalidParams.length, delegatedAgents: 0 });
    const actions = data.research.listActions(data.run.id);
    expect(actions).toHaveLength(invalidParams.length);
    expect(actions.every(action => action.status === "failed")).toBe(true);
  });

  it("Worker 不得再次委派，跨运行和不属于当前运行的需求一律拒绝", async () => {
    const data = setup();
    const worker = createKnowledgeDelegateTool({ ...data.deps, resolveContext: () => ({ ...data.context, role: "worker" as const }) });
    expect((await worker.execute("recursive", { runId: data.run.id, tasks: [data.task()] })).isError).toBe(true);
    const other = data.research.createRun({ turnScopeId: data.scope.id, turnId: data.scope.turnId,
      parentSessionPath: data.scope.sessionPath, question: "另一个研究问题" });
    expect((await data.tool.execute("other-run", { runId: other.id, tasks: [data.task()] })).isError).toBe(true);
    const otherNeed = data.research.createNeed(other.id, { claim: "其它研究的主张", kind: "fact", required: true,
      minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false });
    expect((await data.tool.execute("other-need", { runId: data.run.id,
      tasks: [data.task(), { ...data.task("跨运行"), needIds: [otherNeed.id] }] })).isError).toBe(true);
    expect(data.executeIsolated).not.toHaveBeenCalled();
    expect(data.research.requireRun(data.run.id).delegatedAgents).toBe(0);
  });

  it("Worker 继承宿主来源子集和任务需求子集，参数不能覆盖身份或授予额外权限", async () => {
    const data = setup();
    const context = { ...data.context, allowedSourceIds: [data.sources[0].sourceId], allowedNeedIds: [data.need.id] };
    const tool = createKnowledgeDelegateTool({ ...data.deps, resolveContext: () => context });
    parse(await tool.execute("bounded", { runId: data.run.id, tasks: [data.task()] }));
    const options = data.executeIsolated.mock.calls[0][1];
    expect(options.researchContext).toMatchObject({ actorAgentId: "agent-current", role: "worker",
      allowedSourceIds: [data.sources[0].sourceId], allowedNeedIds: [data.need.id] });
    const denied = createKnowledgeDelegateTool({ ...data.deps, resolveContext: () => ({ ...context, allowedNeedIds: [] }) });
    expect((await denied.execute("denied-need", { runId: data.run.id, tasks: [data.task()] })).isError).toBe(true);
    expect(data.executeIsolated).toHaveBeenCalledTimes(1);
  });

  it("提供者显式失败和异常终止不能被空回答包装成成功", async () => {
    const data = setup();
    data.executeIsolated.mockResolvedValueOnce({ replyText: "供应商错误原文", error: "敏感错误", stopReason: "error" })
      .mockResolvedValueOnce({ replyText: "半截答案", error: null, stopReason: "length" });
    const result = parse(await data.tool.execute("failed-workers", { runId: data.run.id, tasks: [data.task("一"), data.task("二")] }));
    expect(result.status).toBe("failed");
    expect(result.tasks.every((task: { status: string }) => task.status === "failed")).toBe(true);
    expect(JSON.stringify(result)).not.toMatch(/供应商错误原文|敏感错误|半截答案/);
  });
});
