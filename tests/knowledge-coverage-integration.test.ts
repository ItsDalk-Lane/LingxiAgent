/**
 * Knowledge Coverage 集成链路（任务书 §九十六~§一百零三 集成部分，Phase 9 第二波）：
 * buildKnowledgeContextInjection 的 exhaustive 档位端到端——
 * - 集成主链路：exhaustive 计划 → manifest 冻结（真实 KnowledgeStore v14 + TurnScope）
 *   → 全 shard 执行 → gate complete → 注入块含 coverage 状态行/fidelity 行/findings；
 *   普通检索先于 coverage 执行（Priority Planner 被调用），priorityOrder 生效（§六十三）；
 * - §九十八 失败语义：单 shard 持续失败 → partial 措辞 + stats coverageStatus=partial
 *   + 块内无完整性声称；
 * - §九十九 Scope Freeze：执行中产生 V2 snapshot → 本轮 manifest/证据仍 V1；
 * - §一百 共享源去重：Notebook A+B 共享源处理一次（expected units 单份计数）；
 * - §一百零一 Fidelity：needs_ocr 源 → fidelity 行点名 + 不声称原始资料全覆盖；
 * - §八十六 取消 / 超长运行保护：abort → cancelled 如实；runMaxMs 到点 → partial + timeout 留痕；
 * - 执行面降格：workerModel 未配 → 显式降级 broad + 留痕；
 * - §四十一 broad→exhaustive 自动升级（触发与不触发）；
 * - 证据超预算：Phase 10 层级归约压缩（reduceModel）与降级结构化截断 + shard 清单。
 * 纯 injector 真路径：fake workerModel / fake 检索门面 + 真实 KnowledgeStore。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeStore } from "../lib/knowledge/knowledge-store.ts";
import {
  buildKnowledgeContextInjection,
  planCoveragePriorityOrder,
} from "../lib/knowledge/knowledge-context-injector.ts";
import {
  buildCoverageManifest,
  planCoverageShards,
  type CoverageWorkerModel,
} from "../lib/knowledge/knowledge-coverage-manifest.ts";
import type { KnowledgeCoveragePlan } from "../lib/knowledge/knowledge-coverage-planner.ts";
import {
  KNOWLEDGE_COVERAGE_CANCELLED,
  KNOWLEDGE_COVERAGE_PARTIAL,
  KNOWLEDGE_COVERAGE_TIMEOUT,
} from "../shared/knowledge-reason-codes.ts";
import type { DistillModel } from "../lib/knowledge/knowledge-distiller.ts";
import {
  assembleShardEvidenceObjects,
  type CoverageReduceModel,
} from "../lib/knowledge/knowledge-coverage-reduction.ts";
import type {
  NotebookRetrievalChunk,
  NotebookRetrievalSource,
  RetrieveForNotebooksResult,
} from "../lib/knowledge/knowledge-query-service.ts";
import type { KnowledgeBlockDraft } from "../lib/knowledge/source-adapters.ts";

const tempDirs: string[] = [];
const stores: KnowledgeStore[] = [];
const STUDIO = "studio-cov-integration";
let seedCounter = 0;

function createStore() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-coverage-integration-"));
  tempDirs.push(dir);
  const store = new KnowledgeStore({ dbPath: path.join(dir, "knowledge", "knowledge.db") });
  stores.push(store);
  return store;
}

afterEach(() => {
  for (const store of stores.splice(0)) store.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function hashOf(value: string): string {
  let hash = "";
  let seed = 0;
  for (let index = 0; index < value.length; index += 1) seed = (seed * 31 + value.charCodeAt(index)) >>> 0;
  for (let index = 0; index < 64; index += 1) {
    seed = (seed * 1103515245 + 12345) >>> 0;
    hash += (seed % 16).toString(16);
  }
  return hash;
}

function notebook(store: KnowledgeStore, name: string): string {
  return store.createNotebook({ studioId: STUDIO, name }).id;
}

function scopeOf(store: KnowledgeStore, notebookIds: string[]) {
  return store.createTurnScope({
    studioId: STUDIO,
    sessionPath: "/tmp/coverage-integration-test/session.jsonl",
    turnId: `turn-${Math.random().toString(36).slice(2)}`,
    notebookIds,
  });
}

interface SeededSource {
  sourceId: string;
  snapshotId: string;
  artifactId: string;
  sourceName: string;
}

function seedSource(
  store: KnowledgeStore,
  notebookIds: string[],
  options: {
    blockCount?: number;
    blockText?: (index: number) => string;
    locatorType?: "text" | "markdown" | "pdf" | "html";
    status?: "ready" | "needs_ocr";
  } = {},
): SeededSource {
  seedCounter += 1;
  const first = notebookIds[0];
  const blockCount = options.blockCount ?? 2;
  const locatorType = options.locatorType ?? "text";
  const sourceName = `源-${seedCounter}`;
  const imported = store.createSourceWithSnapshot({
    studioId: STUDIO,
    notebookId: first,
    sourceType: "pasted_text",
    displayName: sourceName,
    originMetadata: { kind: "test" },
    snapshot: {
      sha256: hashOf(`snapshot-${seedCounter}-${first}-${Math.random()}`),
      mimeType: "text/plain",
      byteSize: 1024,
      storagePath: `snapshots/snap-${seedCounter}.bin`,
    },
  });
  for (const notebookId of notebookIds.slice(1)) {
    store.addSourceToNotebook({ studioId: STUDIO, notebookId, sourceId: imported.source.id });
  }
  const blocks: KnowledgeBlockDraft[] = options.status === "needs_ocr"
    ? []
    : Array.from({ length: blockCount }, (_, index) => ({
      ordinal: index,
      text: options.blockText ? options.blockText(index) : `第${index}段：集成测试覆盖文本。`,
      locatorType,
      locator: { charStart: 0, charEnd: 16 },
    }));
  const artifact = store.beginParseArtifact({
    studioId: STUDIO,
    contentSnapshotId: imported.snapshot.id,
    parserId: "test-parser",
    parserVersion: "1",
    parserConfigHash: hashOf("parser-config"),
  });
  store.completeParseArtifact({
    studioId: STUDIO,
    parseArtifactId: artifact.id,
    status: options.status ?? "ready",
    warnings: [],
    semanticArtifactPath: `semantic/artifact-${seedCounter}.json`,
    blocks,
  });
  return { sourceId: imported.source.id, snapshotId: imported.snapshot.id, artifactId: artifact.id, sourceName };
}

// ── fake worker / 检索门面（executor 测试同款 prompt 解析） ──

function shardIdOfPrompt(prompt: string): string {
  return /Shard: (\S+) \(ordinal \d+\)/.exec(prompt)![1];
}

function primaryIdsOfPrompt(prompt: string): string[] {
  const match = /Primary units \(scan EVERY one of them\):\n\n([\s\S]*?)(?:\n\nContext after|\n\nReturn exactly)/.exec(prompt);
  expect(match).not.toBeNull();
  return [...match![1].matchAll(/unitId=(cu_[0-9a-f]{64})/gu)].map(entry => entry[1]);
}

function validResultForPrompt(prompt: string, statement?: string): string {
  const primaryBlock = /Primary units \(scan EVERY one of them\):\n\n([\s\S]*?)(?:\n\nContext after|\n\nReturn exactly)/.exec(prompt)![1];
  const header = /sourceId=(\S+) snapshotId=(\S+) parseArtifactId=(\S+)\nblockId=(\S+) startOffset=(\d+) endOffset=(\d+)/.exec(primaryBlock);
  // 默认 statement 带 shard 序号：跨 shard 事实聚合按 statement 归一去重（§八十八），
  // 同文 statement 会合并成单条 finding——测试想看到多条时用 shard 唯一陈述。
  const ordinal = /Shard: \S+ \(ordinal (\d+)\)/.exec(prompt)![1];
  const effectiveStatement = statement ?? `集成测试发现（shard ${ordinal}）：该 shard 覆盖文本要点。`;
  const finding = header
    ? [{
      statement: effectiveStatement,
      support: [{
        sourceId: header[1],
        snapshotId: header[2],
        parseArtifactId: header[3],
        blockId: header[4],
        startOffset: Number(header[5]),
        endOffset: Number(header[6]),
      }],
    }]
    : [];
  return JSON.stringify({
    shardId: shardIdOfPrompt(prompt),
    processedUnitIds: primaryIdsOfPrompt(prompt),
    findings: finding,
    contradictions: [],
    openQuestions: [],
    warnings: [],
  });
}

interface WorkerHarness {
  workerModel: CoverageWorkerModel;
  prompts: string[];
  callLog: string[];
}

function okWorker(callLog: string[] = [], statement?: string, delayMs = 0): WorkerHarness {
  const prompts: string[] = [];
  const workerModel: CoverageWorkerModel = async ({ prompt }) => {
    if (delayMs > 0) await new Promise(resolve => setTimeout(resolve, delayMs));
    callLog.push("worker");
    prompts.push(prompt);
    return validResultForPrompt(prompt, statement);
  };
  return { workerModel, prompts, callLog };
}

/** ~200 CJK 字 ≈ 222 tokens：单 block 单 unit，多块即可跨 shard（16k 预算）。 */
const BIG_BLOCK = (index: number) => `${index}号段落。` + "覆盖测试文本。".repeat(40);

/**
 * 2 源 × 30 大块：~60 units，正文 ≈271 token/块 + 渲染头开销 ≈60 token/块
 * （2026-08-30 开销感知装填后分片按正文+头计）≈ 19.9k 成本 → 稳定切成 2 个
 * shard（49 + 11），保证「一完成一取消/一成功一失败」类用例的结构稳定。
 */
function seedTwoShardSources(store: KnowledgeStore, nb: string) {
  seedSource(store, [nb], { blockCount: 30, blockText: BIG_BLOCK });
  seedSource(store, [nb], { blockCount: 30, blockText: BIG_BLOCK });
}

function retrievalFacade(input: {
  hits: Array<{ seeded: SeededSource; notebookId: string; ordinal?: number; headingPath?: string[] }>;
  sourcesMeta: Array<{ seeded: SeededSource; notebookId: string; sections?: string[]; chunkCount?: number }>;
  callLog?: string[];
}) {
  return async (): Promise<RetrieveForNotebooksResult> => {
    input.callLog?.push("retrieve");
    const candidates: NotebookRetrievalChunk[] = input.hits.map(({ seeded, notebookId, ordinal = 0, headingPath = null }) => ({
      id: `chunk-${seeded.sourceId}-${ordinal}`,
      parseArtifactId: seeded.artifactId,
      chunkIndexVariantId: "civ-test",
      ordinal,
      text: "检索命中的锚点文本。",
      tokenCount: 8,
      spans: [],
      score: 1,
      notebookId,
      notebookName: "资料",
      sourceId: seeded.sourceId,
      sourceName: seeded.sourceName,
      headingPath,
      pageNumber: null,
    } as NotebookRetrievalChunk));
    const sources: NotebookRetrievalSource[] = input.sourcesMeta.map(({ seeded, notebookId, sections, chunkCount }) => ({
      notebookId,
      notebookName: "资料",
      sourceId: seeded.sourceId,
      sourceName: seeded.sourceName,
      parseArtifactId: seeded.artifactId,
      chunkCount: chunkCount ?? 2,
      firstHeadingPath: sections && sections.length > 0 ? [sections[0]] : null,
      ...(sections ? { sections } : {}),
    } as NotebookRetrievalSource));
    return { candidates, sources, retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] };
  };
}

function planOf(overrides: Partial<KnowledgeCoveragePlan> = {}): KnowledgeCoveragePlan {
  return {
    intent: "whole_scope_analysis",
    coverageMode: "exhaustive",
    requiresCompleteness: true,
    scopeLevel: "notebook",
    confidence: 0.9,
    matchedRuleIds: ["RULE_EXHAUSTIVE_KEYWORD"],
    classifierUsed: "rules",
    ...overrides,
  };
}

interface InjectionOptions {
  workerModel: CoverageWorkerModel | null;
  retrieve: (input: { query: string; sourceIds?: string[]; sectionsBySourceId?: ReadonlyMap<string, string[]> }) => Promise<RetrieveForNotebooksResult>;
  scopeId: string;
  coveragePlan?: KnowledgeCoveragePlan | null;
  concurrency?: number;
  signal?: AbortSignal;
  runMaxMs?: number;
  onProgress?: (event: { runId: string; done: number; total: number }) => void;
  budgetTokens?: number;
  distillModel?: DistillModel | null;
  reduceModel?: CoverageReduceModel | null;
}

async function injectWithCoverage(store: KnowledgeStore, options: InjectionOptions) {
  return buildKnowledgeContextInjection({
    question: "请完整梳理全部资料要点，不要遗漏",
    mode: "qa",
    scopeId: options.scopeId,
    ...(options.budgetTokens != null ? { budgetTokens: options.budgetTokens } : {}),
    coveragePlan: options.coveragePlan ?? planOf(),
    deps: {
      decomposeModel: null,
      distillModel: options.distillModel ?? null,
      retrieve: options.retrieve,
      coverage: {
        source: store,
        store,
        studioId: STUDIO,
        workerModel: options.workerModel,
        reduceModel: options.reduceModel ?? null,
        ...(options.concurrency != null ? { concurrency: options.concurrency } : {}),
        ...(options.signal ? { signal: options.signal } : {}),
        ...(options.runMaxMs != null ? { runMaxMs: options.runMaxMs } : {}),
        ...(options.onProgress ? { onProgress: options.onProgress } : {}),
      },
    },
  });
}

// ── 集成主链路（§九十六/§九十七/§六十三） ─────────────────────────────

describe("EXHAUSTIVE 集成主链路", () => {
  it("exhaustive 计划：manifest 冻结 → 全 shard 执行 → complete → 注入块含状态行与 findings；检索先于 coverage 且进度事件带 runId", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    const seededA = seedSource(store, [nb], { blockCount: 3 });
    const seededB = seedSource(store, [nb], { blockCount: 2 });
    const scope = scopeOf(store, [nb]);
    const callLog: string[] = [];
    const { workerModel, prompts } = okWorker(callLog);
    const progress: Array<{ runId: string; done: number; total: number }> = [];
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: retrievalFacade({
        hits: [
          { seeded: seededA, notebookId: nb, ordinal: 0 },
          { seeded: seededB, notebookId: nb, ordinal: 1 },
        ],
        sourcesMeta: [
          { seeded: seededA, notebookId: nb, chunkCount: 3 },
          { seeded: seededB, notebookId: nb, chunkCount: 2 },
        ],
        callLog,
      }),
      scopeId: scope.id,
      concurrency: 1,
      onProgress: event => progress.push(event),
    });

    expect(stats.executedCoverageMode).toBe("exhaustive");
    expect(stats.coverageStatus).toBe("complete");
    expect(stats.coverageRunId).toEqual(expect.stringMatching(/^covrun_/));
    expect(stats.coverageExpectedUnits).toBe(5);
    expect(stats.coverageProcessedUnits).toBe(5);
    expect(stats.coverageFailedUnits).toBe(0);
    expect(stats.coverageShardCompleted).toBe(stats.coverageShardTotal);
    expect(stats.textCoverageRatio).toBe(1);
    expect(stats.coverageFindingsCount).toBeGreaterThan(0);
    expect(stats.coverageReasonCode).toBeUndefined();
    expect(stats).not.toHaveProperty("exhaustivePending");

    // 块内：coverage 状态行（complete 措辞只由 gate 放行）+ findings provenance 头。
    expect(block).toContain("Coverage status: complete — all parseable text in scope has been processed (5 units across 2 sources).");
    expect(block).toContain("Source fidelity: 2 citation_grade. Original-material coverage claim is permitted for this scope.");
    expect(block).toContain("Coverage findings (structured evidence with provenance):");
    expect(block).toMatch(/\[K1\] \[finding ev_[0-9a-f]{16}\] support: sourceId=/);
    // finding 的 support 头指向冻结集合内的真实 artifact（单 shard 时取首个 unit 头，
    // 源顺序由 scope 行序决定，断言归属而非特定源）。
    const artifactInBlock = /parseArtifactId=(\S+)/.exec(block)?.[1];
    expect([seededA.artifactId, seededB.artifactId]).toContain(artifactInBlock);

    // 检索（Priority Planner 输入）先于 coverage worker 执行。
    expect(callLog[0]).toBe("retrieve");
    expect(callLog.slice(1).every(entry => entry === "worker")).toBe(true);

    // 进度事件带 runId、done 递增到 total。
    expect(progress.length).toBeGreaterThan(0);
    expect(progress.every(event => event.runId === stats.coverageRunId)).toBe(true);
    expect(progress[progress.length - 1].done).toBe(stats.coverageShardTotal);
    expect(progress[progress.length - 1].total).toBe(stats.coverageShardTotal);

    // 全部 primary shard 进入终态（§五十一）：prompt 覆盖全部 unit 恰好一次。
    const allUnitIds = prompts.flatMap(prompt => primaryIdsOfPrompt(prompt));
    expect(new Set(allUnitIds).size).toBe(5);
    void prompts;
  });

  it("priorityOrder 生效（§六十三）：执行序等于 Priority Planner 输出，命中源 shard 先扫；全部 shard 仍必达", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    // 两源合计 ~110 units ≈ 24k tokens → 多 shard；检索只命中 B。
    const seededB = seedSource(store, [nb], { blockCount: 90, blockText: BIG_BLOCK });
    const seededA = seedSource(store, [nb], { blockCount: 20, blockText: BIG_BLOCK });
    const scope = scopeOf(store, [nb]);
    const { workerModel, prompts } = okWorker();
    const { stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: retrievalFacade({
        hits: [{ seeded: seededB, notebookId: nb, ordinal: 0 }],
        sourcesMeta: [
          { seeded: seededB, notebookId: nb, chunkCount: 90 },
          { seeded: seededA, notebookId: nb, chunkCount: 20 },
        ],
      }),
      scopeId: scope.id,
      concurrency: 1,
    });

    // 期望序 = planCoveragePriorityOrder（manifest 与执行内构建的同 hash 副本）。
    const manifest = buildCoverageManifest({ source: store, studioId: STUDIO, scopeId: scope.id });
    const fusedChunk = { sourceId: seededB.sourceId } as NotebookRetrievalChunk;
    const expected = planCoveragePriorityOrder({ manifest, fused: [fusedChunk] });
    const executed = prompts.map(prompt => shardIdOfPrompt(prompt));
    expect(executed).toEqual(expected);
    // 全部 shard 必达（低分 shard 不因优先级被裁剪，§五十一/§六十三）。
    expect(stats.coverageStatus).toBe("complete");
    expect(stats.coverageShardTotal).toBeGreaterThan(1);
    expect(stats.coverageShardCompleted).toBe(stats.coverageShardTotal);
  });

  it("planCoveragePriorityOrder 纯函数：命中源的 shard 全部先于未命中源 shard；无命中返回空（退化为 ordinal 序）", () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedSource(store, [nb], { blockCount: 90, blockText: BIG_BLOCK });
    seedSource(store, [nb], { blockCount: 90, blockText: BIG_BLOCK });
    const scope = scopeOf(store, [nb]);
    const manifest = buildCoverageManifest({ source: store, studioId: STUDIO, scopeId: scope.id });
    const plans = planCoverageShards({ manifest });
    expect(plans.length).toBeGreaterThan(1);
    const unitToSource = new Map<string, string>();
    for (const source of manifest.sources) {
      for (const unit of source.coverageUnits) unitToSource.set(unit.id, source.sourceId);
    }
    // 命中 manifest 序的第二个源：其 units 在全局序列靠后（必然落在高 ordinal shard）。
    const hitSourceId = manifest.sources[1].sourceId;
    const shardHasHit = plans.map(plan =>
      plan.primaryUnitIds.some(unitId => unitToSource.get(unitId) === hitSourceId));
    const hitOrdinals = plans.filter((_, index) => shardHasHit[index]).map(plan => plan.ordinal);
    const missOrdinals = plans.filter((_, index) => !shardHasHit[index]).map(plan => plan.ordinal);
    expect(hitOrdinals.length).toBeGreaterThan(0);
    expect(missOrdinals.length).toBeGreaterThan(0);

    const order = planCoveragePriorityOrder({ manifest, fused: [{ sourceId: hitSourceId } as NotebookRetrievalChunk] });
    // 全部命中 shard 先于未命中 shard；同组内按命中 unit 数（分值）降序、并列按 ordinal。
    const hitUnitCounts = new Map(plans.map(plan => [
      plan.shardId,
      plan.primaryUnitIds.filter(unitId => unitToSource.get(unitId) === hitSourceId).length,
    ]));
    const expectedOrder = [
      ...plans.filter((_, index) => shardHasHit[index])
        .sort((left, right) =>
          (hitUnitCounts.get(right.shardId)! - hitUnitCounts.get(left.shardId)!) || (left.ordinal - right.ordinal)),
      ...plans.filter((_, index) => !shardHasHit[index]),
    ].map(plan => plan.shardId);
    expect(order).toEqual(expectedOrder);
    // 命中 shard 先于未命中 shard（位置性质，独立于组内排序复核）。
    const positions = new Map(order.map((shardId, index) => [shardId, index]));
    for (const hit of hitOrdinals) {
      for (const miss of missOrdinals) {
        expect(positions.get(plans[hit].shardId)!).toBeLessThan(positions.get(plans[miss].shardId)!);
      }
    }
    // 无命中：planner 无信息，返回空（executor 退化为 ordinal 序）。
    expect(planCoveragePriorityOrder({ manifest, fused: [] })).toEqual([]);
  });
});

// ── §九十八 失败语义 / §一百零一 Fidelity ─────────────────────────────

describe("EXHAUSTIVE 失败与 fidelity 语义", () => {
  it("§九十八：一个 shard 持续失败 → partial 措辞 + stats coverageStatus=partial + 块内无完整性声称", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedTwoShardSources(store, nb);
    const scope = scopeOf(store, [nb]);
    let callCount = 0;
    const workerModel: CoverageWorkerModel = async ({ prompt }) => {
      callCount += 1;
      // ordinal 1 的 shard 持续输出非法 JSON（纠错重试 + bounded retry 全失败）。
      if (/Shard: \S+ \(ordinal 1\)/.test(prompt)) return "{invalid";
      return validResultForPrompt(prompt);
    };
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] }),
      scopeId: scope.id,
      concurrency: 1,
    });

    expect(stats.coverageStatus).toBe("partial");
    expect(stats.coverageReasonCode).toBe(KNOWLEDGE_COVERAGE_PARTIAL);
    expect(stats.coverageFailedUnits).toBeGreaterThan(0);
    expect(stats.coverageShardFailed).toBe(1);
    expect(stats.textCoverageRatio).toBeLessThan(1);
    // bounded retry 生效：失败 shard 走满 3 次 attempt ×（1+纠错 1）次调用。
    expect(callCount).toBeGreaterThanOrEqual(6);

    expect(block).toContain("Coverage status: partial — processed ");
    expect(block).toContain(`[${KNOWLEDGE_COVERAGE_PARTIAL}]`);
    expect(block).toContain("do NOT claim that the full text has been read");
    // 绝不出现完整性声称（措辞闸）。
    expect(block).not.toContain("all parseable text in scope has been processed");
  });

  it("§一百零一：needs_ocr 源 → fidelity 行点名 + 不声称原始资料全覆盖（text coverage 仍可 complete）", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    const ready1 = seedSource(store, [nb], { blockCount: 2 });
    const ready2 = seedSource(store, [nb], { blockCount: 2 });
    const ocr = seedSource(store, [nb], { status: "needs_ocr" });
    const scope = scopeOf(store, [nb]);
    const { workerModel } = okWorker();
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: retrievalFacade({
        hits: [{ seeded: ready1, notebookId: nb }],
        sourcesMeta: [{ seeded: ready1, notebookId: nb }],
      }),
      scopeId: scope.id,
    });

    // 可解析文本全覆盖（4/4 units）；needs_ocr 零 unit 不进分母。
    expect(stats.coverageStatus).toBe("complete");
    expect(stats.coverageExpectedUnits).toBe(4);
    expect(stats.textCoverageRatio).toBe(1);
    expect(stats.sourceFidelitySummary).toMatchObject({ citation_grade: 2, needs_ocr: 1 });
    // fidelity 行点名 needs_ocr 源，且禁止原始资料全覆盖表述（§五十七/§八十五）。
    expect(block).toContain(`not text-parseable: ${ocr.sourceId} (needs_ocr)`);
    expect(block).toContain("do NOT claim full original-source coverage");
    expect(block).not.toContain("Original-material coverage claim is permitted");
    void ready2;
  });
});

// ── §九十九 Scope Freeze / §一百 共享源去重 ───────────────────────────

describe("EXHAUSTIVE 冻结与共享源", () => {
  it("§九十九：执行中产生 V2 snapshot → 本轮 manifest/证据仍 V1（下一 turn 才见 V2）", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    const v1 = seedSource(store, [nb], { blockCount: 2 });
    const scope = scopeOf(store, [nb]);
    let v2Created = false;
    const workerModel: CoverageWorkerModel = async ({ prompt }) => {
      // 首个 shard 执行中途：watcher 产生 V2（新 snapshot + 新 artifact，内容不同）。
      if (!v2Created) {
        v2Created = true;
        const reimported = store.createSourceWithSnapshot({
          studioId: STUDIO,
          notebookId: nb,
          sourceType: "pasted_text",
          displayName: "源-V2",
          originMetadata: { kind: "test" },
          snapshot: {
            sha256: hashOf(`snapshot-v2-${Math.random()}`),
            mimeType: "text/plain",
            byteSize: 2048,
            storagePath: "snapshots/snap-v2.bin",
          },
        });
        const artifact = store.beginParseArtifact({
          studioId: STUDIO,
          contentSnapshotId: reimported.snapshot.id,
          parserId: "test-parser",
          parserVersion: "1",
          parserConfigHash: hashOf("parser-config"),
        });
        store.completeParseArtifact({
          studioId: STUDIO,
          parseArtifactId: artifact.id,
          status: "ready",
          warnings: [],
          semanticArtifactPath: "semantic/artifact-v2.json",
          blocks: [{ ordinal: 0, text: "V2 新内容，不应进入本轮 manifest。", locatorType: "text", locator: { charStart: 0, charEnd: 16 } }],
        });
      }
      return validResultForPrompt(prompt);
    };
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] }),
      scopeId: scope.id,
      concurrency: 1,
    });

    // 本轮 manifest 冻结在 V1：全部 unit 处理完毕（complete），证据只引用 V1 artifact。
    expect(stats.coverageStatus).toBe("complete");
    expect(stats.coverageExpectedUnits).toBe(2);
    expect(block).toContain(v1.artifactId);
    expect(block).not.toContain("V2 新内容");
    // 下一 turn 的新 scope 才会冻结到 V2（库层保证；链路级复核）。
    const nextScope = scopeOf(store, [nb]);
    const nextManifest = buildCoverageManifest({ source: store, studioId: STUDIO, scopeId: nextScope.id });
    expect(nextManifest.totalCoverageUnits).toBe(3);
  });

  it("§一百：Notebook A+B 共享源处理一次（expected units 单份计数），memberships 双记", async () => {
    const store = createStore();
    const nbA = notebook(store, "A");
    const nbB = notebook(store, "B");
    const shared = seedSource(store, [nbA, nbB], { blockCount: 3 });
    const scope = scopeOf(store, [nbA, nbB]);
    const { workerModel } = okWorker();
    const { stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] }),
      scopeId: scope.id,
    });

    // 共享源 (snapshot, artifact) 去重：处理一次，3 units 只计一份分母。
    expect(stats.coverageExpectedUnits).toBe(3);
    expect(stats.coverageProcessedUnits).toBe(3);
    expect(stats.coverageStatus).toBe("complete");
    // memberships 双记在 manifest 层（库层已测），链路级以同口径复核。
    const manifest = buildCoverageManifest({ source: store, studioId: STUDIO, scopeId: scope.id });
    expect(manifest.sources).toHaveLength(1);
    expect(manifest.sources[0].notebookMemberships.sort()).toEqual([nbA, nbB].sort());
    expect(manifest.sources[0].sourceId).toBe(shared.sourceId);
  });
});

// ── §八十六 取消 / 超长运行保护 / 执行面降格 ──────────────────────────

describe("EXHAUSTIVE 取消、超时与降格", () => {
  it("§八十六：abort 后 run cancelled、stats 如实（cancelled + processed<expected），绝不生成 complete claim", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedTwoShardSources(store, nb);
    const scope = scopeOf(store, [nb]);
    const controller = new AbortController();
    // worker 带 1ms 延迟：确保 abort 拒绝在 Promise.race 中先于同步 resolve 的
    // worker 调用结算（全同步 fake worker 会让 race 先拿到正常结果）。
    const { workerModel } = okWorker([], undefined, 1);
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] }),
      scopeId: scope.id,
      concurrency: 1,
      signal: controller.signal,
      // 首个 shard 完成后（进度回调）触发用户取消。
      onProgress: ({ done }) => {
        if (done >= 1) controller.abort();
      },
    });

    expect(stats.coverageStatus).toBe("cancelled");
    expect(stats.coverageReasonCode).toBe(KNOWLEDGE_COVERAGE_CANCELLED);
    expect(stats.coverageProcessedUnits).toBeLessThan(stats.coverageExpectedUnits);
    expect(stats.coverageShardCompleted).toBe(1);
    expect(stats.coverageShardTotal).toBe(2);
    expect(block).toContain("Coverage status: cancelled — the run was aborted by the user");
    expect(block).toContain(`[${KNOWLEDGE_COVERAGE_CANCELLED}]`);
    expect(block).not.toContain("all parseable text in scope has been processed");
    // 持久化行也如实：run 终态 cancelled。
    const run = store.getCoverageRun({ runId: stats.coverageRunId! });
    expect(run?.run.status).toBe("cancelled");
  });

  it("措辞闸双向生效：全部 shard 完成后才到达的 abort 不剥夺合法 complete claim", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedSource(store, [nb], { blockCount: 2 });
    const scope = scopeOf(store, [nb]);
    const controller = new AbortController();
    const { workerModel } = okWorker();
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] }),
      scopeId: scope.id,
      signal: controller.signal,
      // 单 shard：完成后（done==total）才 abort。
      onProgress: ({ done, total }) => {
        if (done >= total) controller.abort();
      },
    });

    expect(stats.coverageStatus).toBe("complete");
    expect(stats.coverageReasonCode).toBeUndefined();
    expect(block).toContain("Coverage status: complete — all parseable text in scope has been processed");
  });

  it("超长运行保护：runMaxMs 到点 → 取消剩余 shard → partial + timeout 留痕，不无限挂死", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedTwoShardSources(store, nb);
    const scope = scopeOf(store, [nb]);
    const workerModel: CoverageWorkerModel = async ({ prompt }) => {
      await new Promise(resolve => setTimeout(resolve, 80));
      return validResultForPrompt(prompt);
    };
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] }),
      scopeId: scope.id,
      concurrency: 1,
      runMaxMs: 5,
    });

    expect(stats.coverageStatus).toBe("partial");
    expect(stats.coverageReasonCode).toBe(KNOWLEDGE_COVERAGE_TIMEOUT);
    expect(stats.coverageProcessedUnits).toBeLessThan(stats.coverageExpectedUnits);
    expect(block).toContain("exceeded its total time cap");
    expect(block).toContain(`[${KNOWLEDGE_COVERAGE_TIMEOUT}]`);
    expect(block).not.toContain("all parseable text in scope has been processed");
  });

  it("workerModel 未配：显式降级 broad + coverageDegradeReason 留痕（不静默不阻断）", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedSource(store, [nb], { blockCount: 2 });
    const scope = scopeOf(store, [nb]);
    const { block, stats } = await injectWithCoverage(store, {
      workerModel: null,
      retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] }),
      scopeId: scope.id,
    });

    expect(stats.executedCoverageMode).toBe("broad");
    expect(stats.coverageDegradeReason).toBe("coverage worker model not configured");
    expect(stats).not.toHaveProperty("coverageRunId");
    expect(block).toContain("[coverage execution degraded to broad: coverage worker model not configured");
    expect(block).toContain("no completeness claim is made for this turn");
  });
});

// ── §四十一 broad→exhaustive 自动升级 ─────────────────────────────────

describe("broad→exhaustive 自动升级（§四十一 执行侧收口）", () => {
  it("触发：broad 后 sectionCoverageFootprint 仍低于阈值且整体性 scope → 升级 exhaustive 真执行", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedSource(store, [nb], { blockCount: 2 });
    const scope = scopeOf(store, [nb]);
    const { workerModel } = okWorker();
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      // 4 个 section 只命中 1 个：broad section 探测后 footprint = 0.25 < 0.5。
      retrieve: retrievalFacade({
        hits: [{ seeded: { sourceId: "src-x", snapshotId: "snap-x", artifactId: "parse-x", sourceName: "X" }, notebookId: nb, ordinal: 0, headingPath: ["S1"] }],
        sourcesMeta: [{ seeded: { sourceId: "src-x", snapshotId: "snap-x", artifactId: "parse-x", sourceName: "X" }, notebookId: nb, sections: ["S1", "S2", "S3", "S4"], chunkCount: 8 }],
      }),
      scopeId: scope.id,
      coveragePlan: planOf({ coverageMode: "broad", scopeLevel: "notebook" }),
    });

    expect(stats.upgradedTo).toBe("exhaustive");
    expect(stats.executedCoverageMode).toBe("exhaustive");
    expect(stats.sectionCoverageFootprint).toBeLessThan(0.5);
    expect(stats.coverageRunId).toEqual(expect.stringMatching(/^covrun_/));
    expect(stats.coverageStatus).toBe("complete");
    expect(block).toContain("[coverage auto-upgrade: broad → exhaustive");
    expect(block).toContain("Coverage status: complete — all parseable text in scope has been processed");
  });

  it("不触发（scope 非整体性）：source 级 scope 保持 broad，不执行 coverage run", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedSource(store, [nb], { blockCount: 2 });
    const scope = scopeOf(store, [nb]);
    const { workerModel } = okWorker();
    const { stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: retrievalFacade({
        hits: [{ seeded: { sourceId: "src-x", snapshotId: "snap-x", artifactId: "parse-x", sourceName: "X" }, notebookId: nb, ordinal: 0, headingPath: ["S1"] }],
        sourcesMeta: [{ seeded: { sourceId: "src-x", snapshotId: "snap-x", artifactId: "parse-x", sourceName: "X" }, notebookId: nb, sections: ["S1", "S2", "S3", "S4"], chunkCount: 8 }],
      }),
      scopeId: scope.id,
      coveragePlan: planOf({ coverageMode: "broad", scopeLevel: "source" }),
    });

    expect(stats.upgradedTo).toBeUndefined();
    expect(stats.executedCoverageMode).toBe("broad");
    expect(stats).not.toHaveProperty("coverageRunId");
  });

  it("不触发（footprint 达标）：section 覆盖过半不升级", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedSource(store, [nb], { blockCount: 2 });
    const scope = scopeOf(store, [nb]);
    const { workerModel } = okWorker();
    const { stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: retrievalFacade({
        hits: [
          { seeded: { sourceId: "src-x", snapshotId: "snap-x", artifactId: "parse-x", sourceName: "X" }, notebookId: nb, ordinal: 0, headingPath: ["S1"] },
          { seeded: { sourceId: "src-x", snapshotId: "snap-x", artifactId: "parse-x", sourceName: "X" }, notebookId: nb, ordinal: 1, headingPath: ["S2"] },
          { seeded: { sourceId: "src-x", snapshotId: "snap-x", artifactId: "parse-x", sourceName: "X" }, notebookId: nb, ordinal: 2, headingPath: ["S3"] },
        ],
        sourcesMeta: [{ seeded: { sourceId: "src-x", snapshotId: "snap-x", artifactId: "parse-x", sourceName: "X" }, notebookId: nb, sections: ["S1", "S2", "S3", "S4"], chunkCount: 8 }],
      }),
      scopeId: scope.id,
      coveragePlan: planOf({ coverageMode: "broad", scopeLevel: "notebook" }),
    });

    expect(stats.upgradedTo).toBeUndefined();
    expect(stats.executedCoverageMode).toBe("broad");
    expect(stats.sectionCoverageFootprint).toBeGreaterThanOrEqual(0.5);
  });
});

// ── 证据超预算（Phase 10 层级归约 / 降级截断） ─────────────────────────

describe("EXHAUSTIVE 证据超预算", () => {
  it("未配归约模型：结构化截断 + shard 清单留痕 + coverageReduction.degradedReason（不静默）", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedSource(store, [nb], { blockCount: 4, blockText: index => `发现${index}：` + "超预算长发现文本。".repeat(40) });
    const scope = scopeOf(store, [nb]);
    const { workerModel } = okWorker();
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] }),
      scopeId: scope.id,
      budgetTokens: 60,
    });

    expect(stats.coverageStatus).toBe("complete");
    expect(stats.truncated).toBe(true);
    expect(stats.coverageReduction?.degradedReason).toBe("coverage reduce model not configured");
    expect(stats.coverageFindingsCount).toBeGreaterThan(stats.injectedChunks);
    expect(block).toContain("more coverage findings omitted to fit the context budget");
    expect(block).toContain("[coverage reduction degraded: coverage reduce model not configured");
    expect(block).toContain("Shard manifest — the exhaustive scan covered these frozen sources:");
    expect(block).toContain("The scan itself covered every parseable unit");
  });

  it("配了归约模型：层级归约压缩 findings（层级摘要行 + evidence id 留痕）", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    seedSource(store, [nb], { blockCount: 4, blockText: index => `发现${index}：` + "超预算长发现文本。".repeat(40) });
    const scope = scopeOf(store, [nb]);
    // 长 statement（每条 ~660 tokens × 4 > 600 预算）触发 cross 级层级归约。
    const { workerModel } = okWorker([], "超预算发现陈述。".repeat(120));
    const levels: string[] = [];
    const reduceModel: CoverageReduceModel = async ({ prompt }) => {
      const match = /Level: (\S+) evidence reduction/.exec(prompt);
      levels.push(match?.[1] ?? "?");
      // 合法归约：全量合并为一条（id 拼接 + support 并集 + notes verbatim）。
      const marker = "Input evidence (JSON";
      const jsonStart = prompt.indexOf("{", prompt.indexOf(marker));
      const parsed = JSON.parse(prompt.slice(jsonStart, prompt.indexOf("\n", jsonStart)));
      const supports = new Map<string, unknown>();
      for (const finding of parsed.findings) {
        for (const support of finding.support) supports.set(JSON.stringify(support), support);
      }
      return JSON.stringify({
        findings: [{
          id: parsed.findings.map((finding: { id: string }) => finding.id).sort().join("+"),
          statement: `层级归约合并陈述（${parsed.findings.length} 条）。`,
          support: [...supports.values()],
        }],
        contradictions: parsed.contradictions,
        openQuestions: parsed.openQuestions,
        warnings: parsed.warnings,
      });
    };
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] }),
      scopeId: scope.id,
      budgetTokens: 600,
      reduceModel,
    });

    expect(stats.coverageStatus).toBe("complete");
    expect(stats.truncated).toBe(false);
    expect(stats.coverageReduction?.degradedReason).toBeUndefined();
    expect(stats.coverageReduction?.levels.some(level => level.reduced)).toBe(true);
    expect(levels).toContain("cross_notebook");
    expect(block).toContain("evidence object preserved");
    expect(block).toContain("compressed at cross_notebook");
    expect(block).toContain("层级归约合并陈述");
    expect(block).toMatch(/\[K1\] \[finding ev_[0-9a-f]{16}/);
  });

  it("§一百零三大语料端到端：多源多 shard 大量 findings → 注入 token 有界，evidence id 可回溯到 worker 输出", async () => {
    const store = createStore();
    const nb = notebook(store, "A");
    // 3 源 × 40 大块（~222 tokens/块）≈ 27k tokens → 多 shard；worker 每 shard
    // 产出 7 条 ~220 token findings：证据总量远超注入预算（cross 级触发归约）。
    for (let index = 0; index < 3; index += 1) {
      seedSource(store, [nb], { blockCount: 40, blockText: BIG_BLOCK });
    }
    const scope = scopeOf(store, [nb]);
    const workerModel: CoverageWorkerModel = async ({ prompt }) => {
      const shardId = shardIdOfPrompt(prompt);
      const ordinal = /Shard: \S+ \(ordinal (\d+)\)/.exec(prompt)![1];
      const primaryBlock = /Primary units \(scan EVERY one of them\):\n\n([\s\S]*?)(?:\n\nContext after|\n\nReturn exactly)/.exec(prompt)![1];
      const header = /sourceId=(\S+) snapshotId=(\S+) parseArtifactId=(\S+)\nblockId=(\S+) startOffset=(\d+) endOffset=(\d+)/.exec(primaryBlock);
      return JSON.stringify({
        shardId,
        processedUnitIds: primaryIdsOfPrompt(prompt),
        findings: Array.from({ length: 7 }, (_, index) => ({
          statement: `证据点 ${index}（shard ${ordinal}）：` + "大语料集成证据陈述。".repeat(20),
          support: [{
            sourceId: header![1],
            snapshotId: header![2],
            parseArtifactId: header![3],
            blockId: header![4],
            startOffset: Number(header![5]),
            endOffset: Number(header![6]),
          }],
        })),
        contradictions: [],
        openQuestions: [],
        warnings: [],
      });
    };
    const reduceLevels: string[] = [];
    const reduceModel: CoverageReduceModel = async ({ prompt }) => {
      const match = /Level: (\S+) evidence reduction/.exec(prompt);
      reduceLevels.push(match?.[1] ?? "?");
      const marker = "Input evidence (JSON";
      const jsonStart = prompt.indexOf("{", prompt.indexOf(marker));
      const parsed = JSON.parse(prompt.slice(jsonStart, prompt.indexOf("\n", jsonStart)));
      const supports = new Map<string, unknown>();
      for (const finding of parsed.findings) {
        for (const support of finding.support) supports.set(JSON.stringify(support), support);
      }
      return JSON.stringify({
        findings: [{
          id: parsed.findings.map((finding: { id: string }) => finding.id).sort().join("+"),
          statement: `大语料归约合并陈述（${parsed.findings.length} 条证据）。`,
          support: [...supports.values()],
        }],
        contradictions: parsed.contradictions,
        openQuestions: parsed.openQuestions,
        warnings: parsed.warnings,
      });
    };
    const budgetTokens = 600;
    const { block, stats } = await injectWithCoverage(store, {
      workerModel,
      retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] }),
      scopeId: scope.id,
      budgetTokens,
      reduceModel,
    });

    expect(stats.coverageStatus).toBe("complete");
    expect(stats.coverageShardTotal).toBeGreaterThan(1);
    expect(stats.coverageFindingsCount).toBeGreaterThan(7);
    // 注入 token 有界（§一百零三：不能全部一次性进主模型）。
    expect(stats.usedTokens).toBeLessThan(budgetTokens + 200);
    expect(reduceLevels).toContain("cross_notebook");
    expect(stats.coverageReduction?.levels.find(level => level.level === "cross_notebook")?.reduced).toBe(true);

    // 层级摘要行 + evidence id 链回溯到持久化 shard 结果（worker 输出）。
    expect(block).toContain("[reduced: 3 sources → 1 notebook group, 1 evidence object preserved");
    const run = store.getCoverageRun({ runId: stats.coverageRunId! })!;
    const shardResults = run.shards
      .filter(shard => shard.status === "completed" && shard.resultJson != null)
      .map(shard => JSON.parse(shard.resultJson!));
    const inputIds = new Set(assembleShardEvidenceObjects(shardResults).findings.map(finding => finding.id));
    const blockIds = [...block.matchAll(/ev_[0-9a-f]{16}/g)].map(match => match[0]);
    expect(blockIds.length).toBeGreaterThan(0);
    for (const id of blockIds) {
      expect(inputIds.has(id)).toBe(true);
    }
  });
});
