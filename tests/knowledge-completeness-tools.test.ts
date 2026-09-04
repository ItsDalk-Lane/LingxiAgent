import { afterEach, describe, expect, it, vi } from "vitest";
import path from "node:path";
import { KnowledgeError } from "../lib/knowledge/errors.ts";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { KnowledgeCompletenessExecutor, type KnowledgeCompletenessReadUnit } from "../lib/knowledge/research/knowledge-completeness-executor.ts";
import { KnowledgeIndexStore } from "../lib/knowledge/knowledge-index-store.ts";
import { ScopeSnapshotCompiler } from "../lib/knowledge/scope-snapshot-compiler.ts";
import { ResearchToolBudget, type KnowledgeResearchActorContext } from "../lib/knowledge/research/research-tool-budget.ts";
import { createKnowledgeCoverageReadTool } from "../lib/tools/knowledge-coverage-read-tool.ts";
import { createKnowledgeCompletenessMarkTool } from "../lib/tools/knowledge-completeness-mark-tool.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const fixtures: ReturnType<typeof createKnowledgeResearchFixture>[] = [];
afterEach(() => { vi.restoreAllMocks(); for (const fixture of fixtures.splice(0)) fixture.close(); });

function setup() {
  const fixture = createKnowledgeResearchFixture(); fixtures.push(fixture);
  const need = fixture.research.createNeed(fixture.run.id, { claim: "核对完整资料", kind: "completeness", required: true,
    minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: true });
  const context: KnowledgeResearchActorContext = { runId: fixture.run.id, scopeId: fixture.scope.id,
    actorSessionId: "completeness-worker-session", actorAgentId: "research-agent", role: "worker",
    allowedNeedIds: [need.id], allowedSourceIds: fixture.sources.map(source => source.sourceId),
    completenessCheckId: "completeness-check", completenessShardId: "completeness-shard" };
  const source = fixture.sources[0];
  const body = `[unit unit-one]\n${source.text}\n特殊控制字符：\u0001\n[/unit]`;
  const unit = { unitId: "unit-one", sourceId: source.sourceId, contentSnapshotId: source.contentSnapshotId,
    parseArtifactId: source.parseArtifactId, blockId: source.blockId, startOffset: 0, endOffset: source.text.length,
    sectionKey: null, status: "available" as const, receiptId: "receipt-one" };
  const hostSummary = { checkId: context.completenessCheckId!, policy: "scope_complete" as const, status: "running",
    totalUnits: 3, checkedUnits: 1, relevantUnits: 1, unavailableUnits: 0, failedUnits: 0, coverageRatio: 1 / 3,
    exact: false, unavailableSources: [], selectedSectionKeys: [] };
  const completeness = {
    readAssignedShard: vi.fn<KnowledgeCompletenessExecutor["readAssignedShard"]>(() => ({ runId: context.runId,
      checkId: context.completenessCheckId!, shardId: context.completenessShardId!, text: body, units: [unit] })),
    markAssignedUnits: vi.fn<KnowledgeCompletenessExecutor["markAssignedUnits"]>(() => hostSummary),
  };
  const token = {};
  const ledger = new EvidenceLedger(fixture.research), budget = new ResearchToolBudget(fixture.research);
  const deps = { research: fixture.research, ledger, budget, completeness,
    resolveContext: (ctx: unknown) => ctx === token ? context : null };
  const read = createKnowledgeCoverageReadTool(deps), mark = createKnowledgeCompletenessMarkTool(deps);
  const readArgs = { runId: context.runId, checkId: context.completenessCheckId!, shardId: context.completenessShardId! };
  const markArgs = { checkId: context.completenessCheckId!, results: [{ unitId: "unit-one", status: "irrelevant", receiptId: "receipt-one" }] };
  return { ...fixture, need, context, completeness, token, deps, budget, read, mark, readArgs, markArgs, body, unit, hostSummary,
    callRead: (params: Record<string, unknown> = readArgs, ctx: unknown = token, signal?: AbortSignal) => read.execute("read-test", params, signal, undefined, ctx),
    callMark: (params: Record<string, unknown> = markArgs, ctx: unknown = token, signal?: AbortSignal) => mark.execute("mark-test", params, signal, undefined, ctx) };
}

async function executeRealWorker(work: (input: {
  fixture: ReturnType<typeof setup>;
  context: KnowledgeResearchActorContext;
  read: () => ReturnType<ReturnType<typeof setup>["callRead"]>;
  mark: (results: Array<Record<string, unknown>>) => ReturnType<ReturnType<typeof setup>["callMark"]>;
}) => Promise<void>) {
  const fixture = setup();
  fixture.research.upgradeCompletenessPolicy(fixture.run.id, "scope_complete");
  const indexStore = new KnowledgeIndexStore({ dbPath: path.join(path.dirname(fixture.scope.sessionPath), "knowledge-index.db") });
  const compiler = new ScopeSnapshotCompiler({ store: fixture.store, indexStore, requestVariantBuild: () => {} });
  const compiledScope = await compiler.compile(fixture.scope).finally(() => { compiler.dispose(); indexStore.close(); });
  let workerError: unknown;
  const completeness: KnowledgeCompletenessExecutor = new KnowledgeCompletenessExecutor({ research: fixture.research, budget: fixture.budget,
    executeIsolated: async (_prompt, options) => {
      expect(options).toMatchObject({ surface: "knowledge_completeness_worker", permissionMode: "read_only", approvalPolicy: "deny_on_prompt",
        toolFilter: ["knowledge_coverage_read", "knowledge_completeness_mark"] });
      const assignment = options.research as KnowledgeResearchActorContext;
      const context: KnowledgeResearchActorContext = { ...fixture.context, runId: assignment.runId, scopeId: assignment.scopeId,
        allowedNeedIds: assignment.allowedNeedIds, allowedSourceIds: assignment.allowedSourceIds,
        completenessCheckId: assignment.completenessCheckId, completenessShardId: assignment.completenessShardId };
      const token = {}, deps = { ...fixture.deps, completeness, resolveContext: (ctx: unknown) => ctx === token ? context : null };
      const read = createKnowledgeCoverageReadTool(deps), mark = createKnowledgeCompletenessMarkTool(deps);
      try {
        await work({ fixture, context,
          read: () => read.execute("real-read", { runId: context.runId, checkId: context.completenessCheckId,
            shardId: context.completenessShardId }, undefined, undefined, token),
          mark: results => mark.execute("real-mark", { checkId: context.completenessCheckId, results }, undefined, undefined, token) });
      } catch (error) { workerError = error; throw error; }
      return { stopReason: "stop" };
    } });
  const summary = await completeness.ensure({ runId: fixture.run.id, compiledScope, parentSessionId: "active-research-root",
    parentSessionPath: path.join(path.dirname(fixture.scope.sessionPath), "active-research-root.jsonl"), agentId: fixture.context.actorAgentId });
  // 执行器会把工作会话异常记为失败；测试断言仍须在外层重新抛出，不能被这个收尾流程吞掉。
  if (workerError) throw workerError;
  return { ...fixture, completeness, summary };
}

describe("完整性专用工具合同", () => {
  it("阅读参数固定三个身份，标注只允许检查编号和逐单元结果", () => {
    const f = setup();
    expect(Object.keys(f.read.parameters.properties)).toEqual(["runId", "checkId", "shardId"]);
    expect(f.read.parameters.required).toEqual(["runId", "checkId", "shardId"]);
    expect(f.read.parameters).toHaveProperty("additionalProperties", false);
    expect(Object.keys(f.mark.parameters.properties)).toEqual(["checkId", "results"]);
    expect(f.mark.parameters.required).toEqual(["checkId", "results"]);
    expect(f.mark.parameters).toHaveProperty("additionalProperties", false);
    const result = f.mark.parameters.properties.results.items;
    expect(Object.keys(result.properties)).toEqual(["unitId", "status", "receiptId", "evidence"]);
    expect(result.properties.status.anyOf.map(option => option.const)).toEqual(["relevant", "irrelevant", "unavailable"]);
    expect(result).toHaveProperty("additionalProperties", false);
    const evidence = result.properties.evidence.items;
    expect(Object.keys(evidence.properties)).toEqual(["needId", "receiptId", "quote", "occurrenceIndex", "relation", "rationale"]);
    expect(evidence).toHaveProperty("additionalProperties", false);
    expect(f.read.sessionPermission.resolveInvocation()).toEqual({ action: "read", kind: "read", capability: "knowledge_coverage_read.read" });
    expect(f.mark.sessionPermission.resolveInvocation()).toEqual({ action: "read", kind: "read", capability: "knowledge_completeness_mark.read" });
  });

  it("完整阅读正文原样输出且不再次转义，详情和动作只含宿主身份及计数", async () => {
    const f = setup();
    const result = await f.callRead();
    expect(result.isError).toBeUndefined();
    expect(result.content).toEqual([{ type: "text", text: f.body }]);
    expect(result.details).toEqual({ ...f.readArgs, units: [f.unit] });
    expect(f.completeness.readAssignedShard).toHaveBeenCalledExactlyOnceWith(f.context, f.readArgs);
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 1, readCalls: 1 });
    expect(f.research.listActions(f.run.id)[0]).toMatchObject({ actionType: "knowledge_coverage_read",
      status: "completed", requestSummary: {}, responseSummary: { receiptIds: ["receipt-one"], count: 1, status: "completed" } });
    expect(JSON.stringify(result.details)).not.toContain(f.sources[0].text);
    expect(JSON.stringify(f.research.listActions(f.run.id))).not.toContain(f.sources[0].text);
  });

  it("标注原样交宿主校验，返回宿主真实部分进度而非按本批数量自报完成", async () => {
    const f = setup();
    const link = { needId: f.need.id, receiptId: "receipt-one", quote: "九月十五日", occurrenceIndex: 0,
      relation: "supports", rationale: "核对冻结原文的日期" };
    const args = { checkId: f.context.completenessCheckId!, results: [{ unitId: "unit-one", status: "relevant",
      receiptId: "receipt-one", evidence: [link] }] };
    const result = await f.callMark(args);
    expect(result.isError).toBeUndefined();
    expect(JSON.parse(result.content[0].text)).toEqual(f.hostSummary);
    expect(f.completeness.markAssignedUnits).toHaveBeenCalledExactlyOnceWith(f.context, args);
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 1, readCalls: 0 });
    expect(f.research.listActions(f.run.id)[0]).toMatchObject({ actionType: "knowledge_completeness_mark",
      requestSummary: {}, responseSummary: { count: 1, status: "running" }, status: "completed" });
    expect(JSON.stringify(f.research.listActions(f.run.id))).not.toContain(link.quote);
    expect(JSON.stringify(f.research.listActions(f.run.id))).not.toContain(link.rationale);
  });

  it("不可用单元可无凭据标注，工具不会擅自将其计为已检查或exact", async () => {
    const f = setup();
    const summary = { ...f.hostSummary, checkedUnits: 0, relevantUnits: 0, unavailableUnits: 1, coverageRatio: 0 };
    f.completeness.markAssignedUnits.mockReturnValue(summary);
    const result = await f.callMark({ checkId: f.context.completenessCheckId, results: [{ unitId: "unit-one", status: "unavailable" }] });
    expect(JSON.parse(result.content[0].text)).toEqual(summary);
  });

  it("根会话和没有完整性分片分配的普通Worker在扣预算前拒绝", async () => {
    const f = setup();
    f.context.role = "root";
    expect((await f.callRead()).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect((await f.callMark()).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    f.context.role = "worker";
    delete f.context.completenessShardId;
    expect((await f.callRead()).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect((await f.callMark()).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(f.completeness.readAssignedShard).not.toHaveBeenCalled();
    expect(f.completeness.markAssignedUnits).not.toHaveBeenCalled();
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(0);
  });

  it("伪造调用上下文或跨run/check/shard身份不能替代宿主分配", async () => {
    const f = setup();
    expect((await f.callRead(f.readArgs, { ...f.context })).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect((await f.callMark(f.markArgs, { ...f.context })).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    for (const key of ["runId", "checkId", "shardId"]) {
      expect((await f.callRead({ ...f.readArgs, [key]: "another-identity" })).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    }
    expect((await f.callMark({ ...f.markArgs, checkId: "another-check" })).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(0);
    expect(f.research.listActions(f.run.id)).toEqual([]);
  });

  it("阅读调用不能追加单元位置、原文或权限，已授权非法调用照常记录失败预算", async () => {
    const f = setup();
    for (const extras of [{ unitIds: ["other-unit"] }, { startOffset: 1 }, { text: "伪造正文" }, { surface: "knowledge_research_root" }]) {
      expect((await f.callRead({ ...f.readArgs, ...extras })).details).toMatchObject({ errorCode: "KNOWLEDGE_INVALID_ARGUMENT" });
    }
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 4, readCalls: 4 });
    expect(f.research.listActions(f.run.id).every(action => action.status === "failed")).toBe(true);
    expect(f.completeness.readAssignedShard).not.toHaveBeenCalled();
  });

  it("标注未知字段、重复单元、伪造计数和非法引用整批拒绝，不调用宿主写入", async () => {
    const f = setup();
    const result = f.markArgs.results[0];
    const link = { needId: f.need.id, receiptId: "receipt-one", quote: "九月十五日", relation: "supports", rationale: "冻结原文" };
    const bad = [
      { runId: f.run.id }, { exact: true }, { totalUnits: 1 }, { status: "completed" }, { results: null }, { results: [] },
      { results: [result, result] }, { results: [{ ...result, status: "failed" }] }, { results: [{ ...result, status: ["irrelevant"] }] },
      { results: [{ unitId: result.unitId, status: "relevant" }] }, { results: [{ unitId: result.unitId, status: "irrelevant" }] },
      { results: [{ ...result, status: "unavailable", evidence: [] }] }, { results: [{ ...result, exact: true }] },
      { results: [{ ...result, evidence: null }] },
      ...[{ quote: "文".repeat(2001) }, { rationale: "文".repeat(1001) }, { occurrenceIndex: -1 }, { occurrenceIndex: 0.5 },
        { relation: "unknown" }, { sourceId: "forged-source" }, { runId: f.run.id }, { receiptId: "" }]
        .map(override => ({ results: [{ ...result, evidence: [{ ...link, ...override }] }] })),
    ];
    for (const overrides of bad) expect((await f.callMark({ ...f.markArgs, ...overrides })).details)
      .toMatchObject({ errorCode: "KNOWLEDGE_INVALID_ARGUMENT" });
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(bad.length);
    expect(f.research.listActions(f.run.id)).toHaveLength(bad.length);
    expect(f.research.listActions(f.run.id).every(action => action.status === "failed")).toBe(true);
    expect(f.completeness.markAssignedUnits).not.toHaveBeenCalled();
  });

  it.each(["read", "mark"] as const)("%s保留宿主知识错误码，未知异常不泄原文或原始错误", async kind => {
    const f = setup();
    const host = kind === "read" ? f.completeness.readAssignedShard : f.completeness.markAssignedUnits;
    const call = kind === "read" ? f.callRead : f.callMark;
    host.mockImplementationOnce(() => { throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "私密原文MARKER"); });
    expect((await call()).details).toMatchObject({ errorCode: "KNOWLEDGE_MODEL_OUTPUT_INVALID" });
    host.mockImplementationOnce(() => { throw new Error("私密原文MARKER"); });
    const result = await call();
    expect(result.details).toMatchObject({ errorCode: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE" });
    expect(JSON.stringify(result)).not.toContain("私密原文MARKER");
    expect(JSON.stringify(f.research.listActions(f.run.id))).not.toContain("私密原文MARKER");
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(2);
  });

  it("两工具与其他工作会话共用最后一次总预算，第33次不执行也不留下新动作", async () => {
    const f = setup();
    f.store.db.prepare("UPDATE knowledge_research_runs SET tool_calls_used = 31 WHERE id = ?").run(f.run.id);
    expect((await f.callRead()).isError).toBeUndefined();
    expect((await f.callMark()).details).toMatchObject({ errorCode: "KNOWLEDGE_CONFLICT" });
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 32, readCalls: 1, status: "partial", stopReason: "tool_budget_exhausted" });
    expect(f.research.listActions(f.run.id)).toHaveLength(1);
    expect(f.completeness.markAssignedUnits).not.toHaveBeenCalled();
  });

  it("继承研究开始时间，180秒已耗尽不为完整性工具重新计时", async () => {
    const f = setup();
    f.store.db.prepare("UPDATE knowledge_research_runs SET created_at = ? WHERE id = ?")
      .run(new Date(Date.now() - 180_001).toISOString(), f.run.id);
    expect((await f.callRead()).details).toMatchObject({ errorCode: "KNOWLEDGE_CONFLICT" });
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 0, status: "partial", stopReason: "wall_clock_exhausted" });
    expect(f.completeness.readAssignedShard).not.toHaveBeenCalled();
    expect(f.research.listActions(f.run.id)).toEqual([]);
  });

  it("取消后的两工具都不扣预算、不签发凭据、不标注单元", async () => {
    const f = setup(); const controller = new AbortController(); controller.abort();
    await expect(f.callRead(f.readArgs, f.token, controller.signal)).rejects.toThrow();
    await expect(f.callMark(f.markArgs, f.token, controller.signal)).rejects.toThrow();
    expect(f.completeness.readAssignedShard).not.toHaveBeenCalled();
    expect(f.completeness.markAssignedUnits).not.toHaveBeenCalled();
    expect(f.research.requireRun(f.run.id).toolCallsUsed).toBe(0);
  });

  it("真实冻结原文经阅读凭据和引用核验入账，全部Worker退出后才得到exact", async () => {
    const f = await executeRealWorker(async ({ fixture, context, read, mark }) => {
      const result = await read();
      expect(result.isError).toBeUndefined();
      const units = (result.details as { units: KnowledgeCompletenessReadUnit[] }).units;
      expect(units).toHaveLength(2);
      for (const unit of units) {
        const source = fixture.sources.find(source => source.sourceId === unit.sourceId)!;
        expect(result.content[0].text).toContain(source.text);
        const verified = fixture.receipts.read({ runId: fixture.run.id, receiptId: unit.receiptId!, actorSessionId: context.actorSessionId });
        expect(verified.text).toBe(source.text);
        expect(verified.receipt).toMatchObject({ sourceId: unit.sourceId, parseArtifactId: unit.parseArtifactId,
          blockId: unit.blockId, startOffset: unit.startOffset, endOffset: unit.endOffset, channel: "knowledge_read" });
      }
      const marked = await mark(units.map(unit => ({ unitId: unit.unitId, receiptId: unit.receiptId,
        status: unit.sourceId === fixture.sources[0].sourceId ? "relevant" : "irrelevant",
        ...(unit.sourceId === fixture.sources[0].sourceId ? { evidence: [{ needId: fixture.need.id, receiptId: unit.receiptId,
          quote: "九月十五日", relation: "supports", rationale: "原文给出交付日期" }] } : {}) })));
      expect(marked.isError).toBeUndefined();
      expect(JSON.parse(marked.content[0].text)).toMatchObject({ status: "running", checkedUnits: 2, relevantUnits: 1, exact: false });
    });
    expect(f.summary).toMatchObject({ status: "completed", totalUnits: 2, checkedUnits: 2, relevantUnits: 1,
      unavailableUnits: 0, failedUnits: 0, coverageRatio: 1, exact: true });
    expect(f.research.listEvidence(f.run.id).map(evidence => evidence.canonicalText)).toEqual(["九月十五日"]);
    expect(f.store.db.prepare("SELECT * FROM knowledge_completeness_unit_evidence").all()).toHaveLength(1);
    expect(f.research.requireRun(f.run.id)).toMatchObject({ toolCallsUsed: 2, readCalls: 1, delegatedAgents: 1 });
    const metadata = JSON.stringify({ actions: f.research.listActions(f.run.id), coverage: f.store.db.prepare("SELECT manifest_json FROM coverage_runs").all() });
    for (const source of f.sources) expect(metadata).not.toContain(source.text);
  });

  it("真实批次末项伪造引文失败时，前项证据、关系、凭据消费和单元状态全部回滚", async () => {
    const f = await executeRealWorker(async ({ fixture, context, read, mark }) => {
      const readResult = await read();
      expect(readResult.isError).toBeUndefined();
      const units = (readResult.details as { units: KnowledgeCompletenessReadUnit[] }).units;
      const results = units.map((unit, index) => ({ unitId: unit.unitId, status: "relevant", receiptId: unit.receiptId,
        evidence: [{ needId: fixture.need.id, receiptId: unit.receiptId,
          quote: index === 0 ? fixture.sources.find(source => source.sourceId === unit.sourceId)!.text : "原文中不存在的日期",
          relation: "supports", rationale: "原文核对" }] }));
      expect((await mark(results)).details).toMatchObject({ errorCode: "KNOWLEDGE_MODEL_OUTPUT_INVALID" });
      expect(fixture.research.listEvidence(fixture.run.id)).toEqual([]);
      expect(fixture.research.listRelations(fixture.run.id)).toEqual([]);
      expect(fixture.store.db.prepare("SELECT * FROM knowledge_completeness_unit_evidence").all()).toEqual([]);
      expect(fixture.store.db.prepare("SELECT status FROM knowledge_completeness_units WHERE check_id=?")
        .all(context.completenessCheckId).every((row: { status: string }) => row.status === "pending")).toBe(true);
      for (const unit of units) expect(fixture.research.getReceipt(fixture.run.id, unit.receiptId!).consumedAt).toBeNull();
      expect((await mark(units.map(unit => ({ unitId: unit.unitId, status: "irrelevant", receiptId: unit.receiptId })))).isError).toBeUndefined();
    });
    expect(f.summary).toMatchObject({ exact: true, relevantUnits: 0 });
    expect(f.research.listActions(f.run.id).map(action => action.status)).toEqual(["completed", "failed", "completed"]);
  });

  it("真实核验拒绝其他Worker凭据、另一单元凭据和把可读原文谎称不可用", async () => {
    const f = await executeRealWorker(async ({ fixture, read, mark }) => {
      const readResult = await read();
      expect(readResult.isError).toBeUndefined();
      const units = (readResult.details as { units: KnowledgeCompletenessReadUnit[] }).units;
      const unit = units[0];
      const foreign = fixture.receipts.issue({ runId: fixture.run.id, actorSessionId: "another-worker-session",
        sourceId: unit.sourceId, contentSnapshotId: unit.contentSnapshotId, parseArtifactId: unit.parseArtifactId,
        blockId: unit.blockId, startOffset: unit.startOffset, endOffset: unit.endOffset, channel: "knowledge_read" });
      for (const receiptId of [foreign.id, units[1].receiptId]) {
        expect((await mark([{ unitId: unit.unitId, status: "irrelevant", receiptId }])).details)
          .toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
      }
      expect((await mark([{ unitId: unit.unitId, status: "unavailable" }])).details)
        .toMatchObject({ errorCode: "KNOWLEDGE_INVALID_ARGUMENT" });
      expect(fixture.research.listEvidence(fixture.run.id)).toEqual([]);
      expect((await mark(units.map(value => ({ unitId: value.unitId, status: "irrelevant", receiptId: value.receiptId })))).isError).toBeUndefined();
    });
    expect(f.summary).toMatchObject({ exact: true, unavailableUnits: 0 });
  });
});
