import crypto from "node:crypto";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";
import { KnowledgeIndexStore } from "../lib/knowledge/knowledge-index-store.ts";
import { ScopeSnapshotCompiler } from "../lib/knowledge/scope-snapshot-compiler.ts";
import { KnowledgeCompletenessExecutor } from "../lib/knowledge/research/knowledge-completeness-executor.ts";
import { ResearchToolBudget, type KnowledgeResearchActorContext } from "../lib/knowledge/research/research-tool-budget.ts";

type WorkerOptions = Record<string, unknown> & {
  research: KnowledgeResearchActorContext;
  signal?: AbortSignal;
};
const cleanups: Array<() => void> = [];
afterEach(() => { for (const cleanup of cleanups.splice(0)) cleanup(); });
function rawFixture(texts?: string[]) {
  const f = createKnowledgeResearchFixture(texts);
  const index = new KnowledgeIndexStore({ dbPath: path.join(path.dirname(f.scope.sessionPath), "index.db") });
  const compiler = new ScopeSnapshotCompiler({ store: f.store, indexStore: index, requestVariantBuild: () => {} });
  cleanups.push(() => { compiler.dispose(); index.close(); f.close(); });
  return { ...f, compiler };
}
async function runFixture(f: ReturnType<typeof rawFixture>, driver?: (executor: KnowledgeCompletenessExecutor, context: KnowledgeResearchActorContext, options: WorkerOptions) => Promise<unknown>) {
  const scope = f.store.createTurnScope({ studioId: f.studioId, notebookIds: f.scope.notebookIds, sessionPath: f.scope.sessionPath });
  const run = f.research.createRun({ turnScopeId: scope.id, turnId: scope.turnId, parentSessionPath: scope.sessionPath,
    question: "检查整个资料范围有没有遗漏", completenessPolicy: "scope_complete" });
  const need = f.research.createNeed(run.id, { claim: "完整资料核查", kind: "completeness", required: true,
    minIndependentSources: 1, requireCounterEvidence: true, requireAllRelevantUnits: true });
  let counter = 0;
  const executeIsolated = vi.fn(async (_prompt: string, rawOptions: Record<string, unknown>): Promise<unknown> => {
    const options = rawOptions as WorkerOptions;
    const context: KnowledgeResearchActorContext = { ...options.research, role: "worker", actorAgentId: options.agentId as string,
      actorSessionId: `completeness-worker-${++counter}` };
    return driver ? driver(executor, context, options) : mark(executor, context);
  });
  const budget = new ResearchToolBudget(f.research);
  const executor = new KnowledgeCompletenessExecutor({ research: f.research, budget, executeIsolated });
  const input = { runId: run.id, compiledScope: await f.compiler.compile(scope),
    parentSessionId: "host-root", parentSessionPath: path.join(path.dirname(scope.sessionPath), "root.jsonl"), agentId: "agent-a" };
  return { ...f, scope, run, need, budget, executor, executeIsolated, input };
}
function mark(executor: KnowledgeCompletenessExecutor, context: KnowledgeResearchActorContext) {
  const read = executor.readAssignedShard(context, { runId: context.runId, checkId: context.completenessCheckId!, shardId: context.completenessShardId! });
  executor.markAssignedUnits(context, { checkId: read.checkId, results: read.units.map(unit => ({ unitId: unit.unitId,
    status: unit.status === "available" ? "irrelevant" : "unavailable", ...(unit.receiptId ? { receiptId: unit.receiptId } : {}) })) });
  return { stopReason: "stop" };
}
function addUnavailable(f: ReturnType<typeof rawFixture>, status: "unparsed" | "needs_ocr" | "failed") {
  const text = `真实${status}来源`;
  const source = f.store.createSourceWithSnapshot({ studioId: f.studioId, notebookId: f.scope.notebookIds[0],
    sourceType: "pasted_text", displayName: status, originMetadata: {}, snapshot: {
      sha256: crypto.createHash("sha256").update(text).digest("hex"), byteSize: Buffer.byteLength(text), mimeType: "text/plain", storagePath: `sources/${status}.txt` } });
  if (status !== "unparsed") {
    const artifact = f.store.beginParseArtifact({ studioId: f.studioId, contentSnapshotId: source.snapshot.id,
      parserId: "text", parserVersion: "1", parserConfigHash: "b".repeat(64) });
    if (status === "needs_ocr") f.store.completeParseArtifact({ studioId: f.studioId, parseArtifactId: artifact.id,
      status, blocks: [], warnings: ["needs_ocr"], semanticArtifactPath: "parsed/ocr.json" });
    else f.store.failParseArtifact({ studioId: f.studioId, parseArtifactId: artifact.id });
  }
  return source.source.id;
}

describe("不可用来源始终留在完整性分母", () => {
  it("未解析、OCR和失败源没有原文块也各记一项，不伪造unit或省略来源", async () => {
    const raw = rawFixture();
    const unavailableIds = [addUnavailable(raw, "unparsed"), addUnavailable(raw, "needs_ocr"), addUnavailable(raw, "failed")];
    const f = await runFixture(raw);
    const summary = await f.executor.ensure(f.input);
    expect(summary).toMatchObject({ totalUnits: 5, checkedUnits: 2, unavailableUnits: 3, failedUnits: 0,
      coverageRatio: 2 / 5, exact: false, status: "completed" });
    expect(f.store.db.prepare("SELECT status,exact FROM knowledge_completeness_checks WHERE id=?").get(summary!.checkId))
      .toMatchObject({ status: "completed", exact: 0 });
    expect(summary!.unavailableSources.map(source => source.sourceId).sort()).toEqual(unavailableIds.sort());
    expect(f.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_completeness_units").get().count).toBe(2);
    expect(f.store.db.prepare("SELECT DISTINCT source_id FROM knowledge_completeness_units").all().map((row: { source_id: string }) => row.source_id).sort())
      .toEqual(f.sources.map(source => source.sourceId).sort());
    const manifest = JSON.parse(f.store.db.prepare("SELECT manifest_json FROM coverage_runs").get().manifest_json);
    expect(manifest.unavailableSources).toHaveLength(3);
    expect(manifest.units).toHaveLength(2);
    expect(f.executor.isSatisfied(f.run.id, f.need.id)).toBe(false);
  });

  it("所有来源都不可用时零Worker，分母仍非零且绝不exact", async () => {
    const raw = rawFixture([]);
    addUnavailable(raw, "unparsed"); addUnavailable(raw, "needs_ocr");
    const f = await runFixture(raw);
    expect(await f.executor.ensure(f.input)).toMatchObject({ totalUnits: 2, checkedUnits: 0, unavailableUnits: 2, coverageRatio: 0, exact: false, status: "completed" });
    expect(f.executeIsolated).not.toHaveBeenCalled();
  });

  it("分母冻结后原文运行时不可读，保留原unit并如实标不可用", async () => {
    const raw = rawFixture();
    const f = await runFixture(raw, async (executor, context) => {
      // 模拟读取窗口内持久化状态故障；不能换成其它ready产物或照读缓存证明完整。
      f.store.db.prepare("UPDATE parse_artifacts SET status='needs_ocr' WHERE id=?").run(f.sources[0].parseArtifactId);
      const result = executor.readAssignedShard(context, { runId: context.runId, checkId: context.completenessCheckId!, shardId: context.completenessShardId! });
      expect(result.units.find(unit => unit.sourceId === f.sources[0].sourceId)).toMatchObject({ status: "unavailable" });
      expect(result.text).not.toContain(f.sources[0].text);
      executor.markAssignedUnits(context, { checkId: result.checkId, results: result.units.map(unit => ({ unitId: unit.unitId,
        status: unit.status === "available" ? "irrelevant" : "unavailable", ...(unit.receiptId ? { receiptId: unit.receiptId } : {}) })) });
      return { stopReason: "stop" };
    });
    expect(await f.executor.ensure(f.input)).toMatchObject({ totalUnits: 2, checkedUnits: 1, unavailableUnits: 1, exact: false, failedUnits: 0 });
    expect(f.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_completeness_units").get().count).toBe(2);
  });

  it("完成后原文hash不再匹配，读摘要也不能沿用过期exact", async () => {
    const f = await runFixture(rawFixture());
    expect(await f.executor.ensure(f.input)).toMatchObject({ exact: true });
    f.store.db.prepare("UPDATE knowledge_blocks SET text='被外部破坏的正文' WHERE id=?").run(f.sources[0].blockId);
    expect(f.executor.getSummary(f.run.id)).toMatchObject({ totalUnits: 2, checkedUnits: 1, unavailableUnits: 1, exact: false });
    expect(f.executor.isSatisfied(f.run.id)).toBe(false);
  });

  it("共享4个Worker上限，取消必须等待所有实际清理并保留未检查分母", async () => {
    const raw = rawFixture(["大范围原文".repeat(12000)]);
    const controller = new AbortController();
    let active = 0, maxActive = 0, cleaned = 0, release!: () => void;
    const cleanupGate = new Promise<void>(resolve => { release = resolve; });
    const f = await runFixture(raw, async (_executor, _context, options) => {
      active++; maxActive = Math.max(maxActive, active);
      await new Promise<void>(resolve => options.signal!.addEventListener("abort", () => resolve(), { once: true }));
      await cleanupGate; active--; cleaned++;
      return { error: "aborted" };
    });
    let settled = false;
    const pending = f.budget.withRunController(f.run.id, controller.signal, signal => f.executor.ensure({ ...f.input, signal }))
      .then(() => { settled = true; }, () => { settled = true; });
    await vi.waitFor(() => expect(f.executeIsolated).toHaveBeenCalledTimes(4));
    expect(maxActive).toBe(4);
    controller.abort(new DOMException("取消", "AbortError"));
    await new Promise(resolve => setImmediate(resolve));
    expect(settled).toBe(false); expect(cleaned).toBe(0);
    release(); await pending;
    expect(cleaned).toBe(4); expect(active).toBe(0);
    expect(f.executeIsolated).toHaveBeenCalledTimes(4);
    const summary = f.executor.getSummary(f.run.id)!;
    expect(summary.totalUnits).toBeGreaterThan(4);
    expect(summary.exact).toBe(false);
    expect(summary.status).toBe("cancelled");
    expect(f.store.db.prepare("SELECT COUNT(*) AS count FROM coverage_shards WHERE status='running'").get().count).toBe(0);
  });
});
