/**
 * Knowledge Coverage Executor（任务书 §九十六–§一百零三 库层部分，Phase 9 第一波）：
 * - CoverageUnit：超预算 block 切分后区间不重叠不遗漏（§四十五/§一百零三）；
 * - Manifest：共享 Source 去重（处理次数=1、memberships=[A,B]，§一百）、fidelity
 *   单列（3 ready + 1 needs_ocr：text coverage 完整但绝不返回 full original
 *   coverage，§一百零一）、确定性（同输入两次 manifestHash/shard 边界一致，§四十八）；
 * - Executor：100 units 恰好计数一次、context overlap 不进分母（§九十七）；
 *   99+1 failed → coverageStatus != complete（§九十八）；worker 无发现返回空
 *   findings 且 processedUnitIds 完整（§五十二）；恢复（completed 不重跑、pending
 *   续跑、running 置回 pending，§六十五）；取消语义（§八十六）；并发池上限
 *   （§八十七）；shard 失败重试上限与 attempt 内纠错重试。
 * 纯库层：fake workerModel + 真实 KnowledgeStore（temp db，schema v14）。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KnowledgeStore } from "../lib/knowledge/knowledge-store.ts";
import {
  COVERAGE_CIRCUIT_BREAK_FAILURES,
  COVERAGE_EXECUTOR_DEFAULT_CONCURRENCY,
  COVERAGE_SHARD_MAX_ATTEMPTS,
  computeCoverageLedger,
  evaluateCoverageGate,
  executeCoverageRun,
  fidelityAllowsOriginalCoverageClaim,
  type CoverageExecutionResult,
} from "../lib/knowledge/knowledge-coverage-executor.ts";
import {
  COVERAGE_SHARD_CONTEXT_UNITS,
  COVERAGE_SHARD_TOKEN_BUDGET,
  aggregateShardEvidence,
  buildCoverageManifest,
  buildShardWorkerPrompt,
  manifestUnitSequence,
  parseShardResult,
  planCoverageShards,
  shardKnownBlocks,
  type CoverageManifest,
  type CoverageShardPlan,
  type CoverageWorkerModel,
  type ShardResult,
} from "../lib/knowledge/knowledge-coverage-manifest.ts";
import {
  COVERAGE_UNIT_TOKEN_BUDGET,
  buildCoverageUnits,
  verifyCoverageUnits,
} from "../lib/knowledge/knowledge-coverage-unit.ts";
import {
  KNOWLEDGE_COVERAGE_CIRCUIT_BREAK,
  KNOWLEDGE_COVERAGE_PARTIAL,
  KNOWLEDGE_COVERAGE_SHARD_FAILED,
} from "../shared/knowledge-reason-codes.ts";
import type { KnowledgeBlockDraft } from "../lib/knowledge/source-adapters.ts";
import { estimateTextTokens } from "../lib/llm/estimate-text-tokens.ts";

const tempDirs: string[] = [];
const stores: KnowledgeStore[] = [];
const STUDIO = "studio-cov";
let seedCounter = 0;

function createStore() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-coverage-executor-"));
  tempDirs.push(dir);
  const store = new KnowledgeStore({ dbPath: path.join(dir, "knowledge", "knowledge.db") });
  stores.push(store);
  return store;
}

afterEach(() => {
  for (const store of stores.splice(0)) store.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

/** 任意 64 hex（store 只校验形状，不比对内容）。 */
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

function scopeOf(store: KnowledgeStore, notebookIds: string[]): string {
  return store.createTurnScope({
    studioId: STUDIO,
    sessionPath: "/tmp/coverage-executor-test/session.jsonl",
    turnId: `turn-${Math.random().toString(36).slice(2)}`,
    notebookIds,
  }).id;
}

interface SeededSource {
  sourceId: string;
  snapshotId: string;
  artifactId: string;
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
  const blockCount = options.blockCount ?? 1;
  const locatorType = options.locatorType ?? "text";
  const imported = store.createSourceWithSnapshot({
    studioId: STUDIO,
    notebookId: first,
    sourceType: "pasted_text",
    displayName: `源-${seedCounter}`,
    originMetadata: { kind: "test" },
    snapshot: {
      sha256: hashOf(`snapshot-${seedCounter}-${first}`),
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
      text: options.blockText ? options.blockText(index) : `第${index}段：默认测试文本内容。`,
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
  return {
    sourceId: imported.source.id,
    snapshotId: imported.snapshot.id,
    artifactId: artifact.id,
  };
}

const PLAN_SUMMARY = {
  intent: "whole_scope_analysis",
  coverageMode: "exhaustive",
  scopeLevel: "notebook",
  subQueries: ["全部要点", "有无矛盾"],
};

function shardIdOfPrompt(prompt: string): string {
  return /Shard: (\S+) \(ordinal \d+\)/.exec(prompt)![1];
}

function primaryIdsOfPrompt(prompt: string): string[] {
  const match = /Primary units \(scan EVERY one of them\):\n\n([\s\S]*?)(?:\n\nContext after|\n\nReturn exactly)/.exec(prompt);
  expect(match).not.toBeNull();
  return [...match![1].matchAll(/unitId=(cu_[0-9a-f]{64})/gu)].map(entry => entry[1]);
}

/** 按 prompt 解析 shardId 与 primary units，产出合法 ShardResult（fake worker）。 */
function validResultForPrompt(prompt: string): string {
  const primaryBlock = /Primary units \(scan EVERY one of them\):\n\n([\s\S]*?)(?:\n\nContext after|\n\nReturn exactly)/.exec(prompt)![1];
  const header = /sourceId=(\S+) snapshotId=(\S+) parseArtifactId=(\S+)\nblockId=(\S+) startOffset=(\d+) endOffset=(\d+)/.exec(primaryBlock);
  const finding = header
    ? [{
      statement: "测试发现：覆盖文本要点。",
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

function okWorker(): CoverageWorkerModel & { prompts: string[] } {
  const prompts: string[] = [];
  const worker: CoverageWorkerModel = async ({ prompt }) => {
    prompts.push(prompt);
    return validResultForPrompt(prompt);
  };
  return Object.assign(worker, { prompts });
}

async function runCoverage(input: {
  store: KnowledgeStore;
  manifest: CoverageManifest;
  workerModel: CoverageWorkerModel;
  question?: string;
  concurrency?: number;
  shardTokenBudget?: number;
  signal?: AbortSignal;
  priorityOrder?: string[];
  onProgress?: (done: number, total: number) => void;
}): Promise<CoverageExecutionResult> {
  return executeCoverageRun({
    store: input.store,
    manifest: input.manifest,
    question: input.question ?? "请完整梳理全部资料要点，不要遗漏",
    planSummary: PLAN_SUMMARY,
    workerModel: input.workerModel,
    ...(input.concurrency != null ? { concurrency: input.concurrency } : {}),
    ...(input.shardTokenBudget != null ? { shardTokenBudget: input.shardTokenBudget } : {}),
    ...(input.signal ? { signal: input.signal } : {}),
    ...(input.priorityOrder ? { priorityOrder: input.priorityOrder } : {}),
    ...(input.onProgress ? { onProgress: input.onProgress } : {}),
  });
}

/** ~201 CJK 字 ≈ 222 tokens：单 block 单 unit，多个 block 即多个 unit。 */
const BIG_BLOCK = (index: number) => `${index}号段落。` + "覆盖测试文本。".repeat(40);

function manifestWithUnits(store: KnowledgeStore, unitCount: number): CoverageManifest {
  const notebookA = notebook(store, "A");
  seedSource(store, [notebookA], { blockCount: unitCount, blockText: BIG_BLOCK });
  return buildCoverageManifest({
    source: store,
    studioId: STUDIO,
    scopeId: scopeOf(store, [notebookA]),
  });
}

// ─────────────────────── CoverageUnit（§四十五/§一百零三） ───────────────────────

describe("CoverageUnit 切分", () => {
  it("超预算 block 切成多个 unit：区间两两不交、并集=全集（§一百零三）", () => {
    const paragraphs = Array.from({ length: 200 }, (_, index) => `第${index}段落的内容，用于撑爆单 unit 预算。`);
    const bigText = paragraphs.join("\n\n");
    const blocks = [
      { id: "block_a", ordinal: 0, text: "短块。", parseArtifactId: "parse_x", locatorType: "text", locator: {} } as any,
      { id: "block_b", ordinal: 1, text: bigText, parseArtifactId: "parse_x", locatorType: "text", locator: {} } as any,
      { id: "block_c", ordinal: 2, text: "尾部短块。", parseArtifactId: "parse_x", locatorType: "text", locator: {} } as any,
    ];
    const units = buildCoverageUnits({
      sourceId: "src_1",
      parseArtifactId: "parse_x",
      blocks,
      unitTokenBudget: 2048,
    });
    const bigUnits = units.filter(unit => unit.blockId === "block_b");
    expect(bigUnits.length).toBeGreaterThan(1);
    for (const unit of bigUnits) expect(unit.tokenEstimate).toBeLessThanOrEqual(2048);
    // 软边界优先落在 \n\n 上。
    expect(bigUnits.some(unit => unit.text.endsWith("\n\n"))).toBe(true);
    // 不变量的机器证明：无 gap、无 overlap。
    const verification = verifyCoverageUnits(units, blocks.map(block => ({ ordinal: block.ordinal, text: block.text })));
    expect(verification.exact).toBe(true);
    expect(verification.gaps).toEqual([]);
    expect(verification.overlaps).toEqual([]);
    // 确定性 id：同输入重建必同 id 序列。
    const rebuilt = buildCoverageUnits({
      sourceId: "src_1", parseArtifactId: "parse_x", blocks, unitTokenBudget: 2048,
    });
    expect(rebuilt.map(unit => unit.id)).toEqual(units.map(unit => unit.id));
  });

  it("预算内 block 一 block 一 unit；默认预算常量为 2048", () => {
    const blocks = [
      { id: "block_a", ordinal: 0, text: "第一段。", parseArtifactId: "parse_x", locatorType: "text", locator: {} } as any,
      { id: "block_b", ordinal: 1, text: "第二段。", parseArtifactId: "parse_x", locatorType: "text", locator: {} } as any,
    ];
    const units = buildCoverageUnits({ sourceId: "src_1", parseArtifactId: "parse_x", blocks });
    expect(COVERAGE_UNIT_TOKEN_BUDGET).toBe(2048);
    expect(units).toHaveLength(2);
    expect(units[0]).toMatchObject({ blockId: "block_a", startOffset: 0, endOffset: 4 });
  });
});

// ─────────────────── Manifest / Sharding（§四十六–§四十八/§一百/§一百零一） ───────────────────

describe("CoverageManifest 构建与确定性", () => {
  it("共享 Source 去重：同 snapshot+artifact 只一条，memberships=[A,B]（§一百）", () => {
    const store = createStore();
    const notebookA = notebook(store, "A");
    const notebookB = notebook(store, "B");
    const shared = seedSource(store, [notebookA, notebookB], { blockCount: 2 });
    const scopeId = scopeOf(store, [notebookA, notebookB]);

    const manifest = buildCoverageManifest({ source: store, studioId: STUDIO, scopeId });
    expect(manifest.totalSources).toBe(1);
    expect(manifest.sources[0].sourceId).toBe(shared.sourceId);
    expect(manifest.sources[0].notebookMemberships).toEqual([notebookA, notebookB]);
    expect(manifest.totalCoverageUnits).toBe(2);
    expect(manifest.sourceFidelitySummary).toMatchObject({ citation_grade: 1, needs_ocr: 0 });
  });

  it("fidelity：html → structural；needs_ocr 单列且零 unit（§五十九/§一百零一）", () => {
    const store = createStore();
    const notebookA = notebook(store, "A");
    seedSource(store, [notebookA], { blockCount: 1, locatorType: "html" });
    seedSource(store, [notebookA], { blockCount: 1, status: "needs_ocr" });
    const scopeId = scopeOf(store, [notebookA]);

    const manifest = buildCoverageManifest({ source: store, studioId: STUDIO, scopeId });
    expect(manifest.totalSources).toBe(2);
    expect(manifest.sources.some(source => source.fidelity === "structural")).toBe(true);
    const needsOcr = manifest.sources.find(source => source.fidelity === "needs_ocr")!;
    expect(needsOcr.coverageUnits).toEqual([]);
    expect(manifest.totalCoverageUnits).toBe(1);
    expect(manifest.sourceFidelitySummary).toMatchObject({ structural: 1, needs_ocr: 1 });
  });

  it("同输入两次构建：manifestHash 与 shard 边界完全一致（§四十八）", () => {
    const store = createStore();
    const notebookA = notebook(store, "A");
    seedSource(store, [notebookA], { blockCount: 6, blockText: BIG_BLOCK });
    const scopeId = scopeOf(store, [notebookA]);

    const first = buildCoverageManifest({ source: store, studioId: STUDIO, scopeId });
    const second = buildCoverageManifest({ source: store, studioId: STUDIO, scopeId });
    expect(second.manifestHash).toBe(first.manifestHash);
    expect(COVERAGE_SHARD_TOKEN_BUDGET).toBe(16384);
    const firstShards = planCoverageShards({ manifest: first, shardTokenBudget: 300 });
    const secondShards = planCoverageShards({ manifest: second, shardTokenBudget: 300 });
    expect(firstShards.map(shard => shard.shardId)).toEqual(secondShards.map(shard => shard.shardId));
    expect(firstShards.map(shard => shard.primaryUnitIds)).toEqual(secondShards.map(shard => shard.primaryUnitIds));
  });

  it("context 窗口：相邻 shard 首尾各 ≤2 units；primary 全集恰一次（§四十九）", () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 12);
    const shards = planCoverageShards({ manifest, shardTokenBudget: 300 });
    expect(shards.length).toBeGreaterThan(2);
    for (let index = 1; index < shards.length; index += 1) {
      const previous = shards[index - 1].primaryUnitIds;
      expect(shards[index].contextBeforeUnitIds)
        .toEqual(previous.slice(Math.max(0, previous.length - COVERAGE_SHARD_CONTEXT_UNITS)));
    }
    expect(shards[0].contextBeforeUnitIds).toEqual([]);
    expect(shards[shards.length - 1].contextAfterUnitIds).toEqual([]);
    const allPrimary = shards.flatMap(shard => shard.primaryUnitIds);
    expect(new Set(allPrimary).size).toBe(allPrimary.length);
    expect(allPrimary.length).toBe(manifest.totalCoverageUnits);
    // context 只是相邻引用，不改变 primary 分区。
    const contextIds = new Set(shards.flatMap(shard => [...shard.contextBeforeUnitIds, ...shard.contextAfterUnitIds]));
    expect(contextIds.size).toBeGreaterThan(0);
  });

  it("scope 不存在 / 跨 studio 显式拒绝", () => {
    const store = createStore();
    expect(() => buildCoverageManifest({ source: store, studioId: STUDIO, scopeId: "kts_missing" }))
      .toThrow(/Turn scope not found/);
    const notebookA = notebook(store, "A");
    seedSource(store, [notebookA]);
    const scopeId = scopeOf(store, [notebookA]);
    expect(() => buildCoverageManifest({ source: store, studioId: "studio-other", scopeId }))
      .toThrow(/another studio/);
  });
});

// ─────────────── 执行：完整性 / 失败 / 恢复 / 取消（§五十二/§九十七–§九十九） ───────────────

describe("executeCoverageRun", () => {
  it("100 units 恰好计数一次；context overlap 不进分母（§九十七）", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 100);
    expect(manifest.totalCoverageUnits).toBe(100);
    const worker = okWorker();
    const result = await runCoverage({ store, manifest, workerModel: worker, concurrency: 4, shardTokenBudget: 2000 });

    expect(result.runStatus).toBe("complete");
    expect(result.ledger.expectedPrimaryUnits).toBe(100);
    expect(result.ledger.processedPrimaryUnits).toBe(100);
    expect(result.ledger.failedPrimaryUnits).toBe(0);
    expect(result.ledger.skippedPrimaryUnits).toBe(0);
    expect(result.gate.coverageStatus).toBe("complete");
    expect(result.gate.allowedClaim).toBe("full_text_processed");
    expect(result.gate.textCoverageRatio).toBe(1);
    // 分片 > 1 → prompt 里含 context 重叠 unit，但分母仍是 100。
    const shards = planCoverageShards({ manifest, shardTokenBudget: 2000 });
    expect(shards.length).toBeGreaterThan(1);
    const contextIds = new Set(shards.flatMap(shard => [...shard.contextBeforeUnitIds, ...shard.contextAfterUnitIds]));
    expect(contextIds.size).toBeGreaterThan(0);
    // 每个 unit 恰好被一个 shard 记 primary（并集=全集且两两不交）。
    const primaryCount = new Map<string, number>();
    for (const shard of shards) {
      for (const unitId of shard.primaryUnitIds) primaryCount.set(unitId, (primaryCount.get(unitId) ?? 0) + 1);
    }
    expect(primaryCount.size).toBe(100);
    for (const unit of manifestUnitSequence(manifest)) {
      expect(primaryCount.get(unit.id)).toBe(1);
    }
  });

  it("worker 无发现：空 findings + processedUnitIds 全列也是完成结果（§五十二）", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 3);
    const worker: CoverageWorkerModel = async ({ prompt }) => JSON.stringify({
      shardId: shardIdOfPrompt(prompt),
      processedUnitIds: primaryIdsOfPrompt(prompt),
      findings: [],
      contradictions: [],
      openQuestions: [],
      warnings: [],
    });
    const result = await runCoverage({ store, manifest, workerModel: worker });
    expect(result.runStatus).toBe("complete");
    expect(result.ledger.processedPrimaryUnits).toBe(3);
    expect(result.shardResults.every(shard => shard.findings.length === 0)).toBe(true);
    expect(result.evidence.findings).toEqual([]);
  });

  it("99+1 failed：coverageStatus != complete、只能 partial 措辞（§九十八）", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 100);
    const shards = planCoverageShards({ manifest, shardTokenBudget: 2000 });
    const victim = shards[shards.length - 1].shardId;
    const worker: CoverageWorkerModel = async ({ prompt }) =>
      shardIdOfPrompt(prompt) === victim ? "这不是 JSON" : validResultForPrompt(prompt);
    const result = await runCoverage({ store, manifest, workerModel: worker, concurrency: 2, shardTokenBudget: 2000 });

    expect(result.runStatus).toBe("partial");
    expect(result.gate.coverageStatus).toBe("partial");
    expect(result.gate.allowedClaim).toBe("partial_only");
    expect(result.gate.textCoverageRatio).toBeLessThan(1);
    expect(result.ledger.processedPrimaryUnits).toBe(100 - shards[shards.length - 1].primaryUnitIds.length);
    expect(result.ledger.failedPrimaryUnits).toBe(shards[shards.length - 1].primaryUnitIds.length);
    expect(result.reasonCode).toBe(KNOWLEDGE_COVERAGE_PARTIAL);
    expect(result.failedShards).toHaveLength(1);
    expect(result.failedShards[0].lastError).toContain(KNOWLEDGE_COVERAGE_SHARD_FAILED);
    expect(result.failedShards[0].attempts).toBe(COVERAGE_SHARD_MAX_ATTEMPTS);
  });

  it("shard 失败重试上限：attempt 内纠错一次 × bounded retry 2 次后终态 failed", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 2);
    let calls = 0;
    const worker: CoverageWorkerModel = async () => {
      calls += 1;
      return "garbage";
    };
    const result = await runCoverage({ store, manifest, workerModel: worker, shardTokenBudget: 10 });
    expect(result.runStatus).toBe("failed");
    expect(result.ledger.processedPrimaryUnits).toBe(0);
    expect(result.ledger.failedPrimaryUnits).toBe(2);
    expect(calls).toBe(2 /* shards */ * COVERAGE_SHARD_MAX_ATTEMPTS * 2 /* 纠错重试 */);
    const state = store.getCoverageRun({ runId: result.runId })!;
    for (const shard of state.shards) {
      expect(shard.status).toBe("failed");
      expect(shard.attemptCount).toBe(COVERAGE_SHARD_MAX_ATTEMPTS);
    }
    expect(state.run.failedUnits).toBe(2);
  });

  it("processedUnitIds 契约：漏列 unit id → 输出非法 → 纠错重试一次后成功", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 4);
    const worker = vi.fn(async ({ prompt }: { prompt: string }) => {
      const valid = validResultForPrompt(prompt);
      if (worker.mock.calls.length === 1) {
        const parsed = JSON.parse(valid);
        parsed.processedUnitIds = parsed.processedUnitIds.slice(0, -1);
        return JSON.stringify(parsed);
      }
      return valid;
    });
    const result = await runCoverage({ store, manifest, workerModel: worker as unknown as CoverageWorkerModel });
    expect(result.runStatus).toBe("complete");
    const correctionCalls = worker.mock.calls.filter(call => (call[0] as any).correction != null);
    expect(correctionCalls).toHaveLength(1);
    expect((correctionCalls[0][0] as any).correction.error).toContain("processedUnitIds");
    expect((correctionCalls[0][0] as any).correction.previousOutput).toContain("shardId");
  });

  it("并发池上限：峰值 active ≤ concurrency（§八十七）", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 40);
    let active = 0;
    let peak = 0;
    const worker: CoverageWorkerModel = async ({ prompt }) => {
      active += 1;
      peak = Math.max(peak, active);
      await new Promise(resolve => setTimeout(resolve, 5));
      active -= 1;
      return validResultForPrompt(prompt);
    };
    const result = await runCoverage({
      store, manifest, workerModel: worker, concurrency: 2, shardTokenBudget: 400,
    });
    expect(result.runStatus).toBe("complete");
    expect(peak).toBe(2);
    expect(peak).toBeLessThanOrEqual(COVERAGE_EXECUTOR_DEFAULT_CONCURRENCY);
  });

  it("priorityOrder 只改顺序不改必达性（§六十三）", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 20);
    const shards = planCoverageShards({ manifest, shardTokenBudget: 300 });
    expect(shards.length).toBeGreaterThan(2);
    const reversed = [...shards.map(shard => shard.shardId)].reverse();
    const order: string[] = [];
    const worker: CoverageWorkerModel = async ({ prompt }) => {
      order.push(shardIdOfPrompt(prompt));
      return validResultForPrompt(prompt);
    };
    const result = await runCoverage({
      store, manifest, workerModel: worker, concurrency: 1,
      shardTokenBudget: 300, priorityOrder: reversed,
    });
    expect(result.runStatus).toBe("complete");
    expect(result.ledger.processedPrimaryUnits).toBe(20);
    // 串行（concurrency 1）下执行序 = priorityOrder（全部 shard 都被点名）。
    expect(order).toEqual(reversed);
  });

  it("恢复：completed shard 不重跑（复用 result_json）、pending 续跑、running 置回 pending（§六十五）", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 30);
    const plans = planCoverageShards({ manifest, shardTokenBudget: 300 });
    expect(plans.length).toBeGreaterThan(3);

    // 直接落一个"进程中断"现场：shard0 已完成，shard1 卡 running，其余 pending。
    const created = store.createCoverageRun({
      turnScopeId: manifest.turnScopeId,
      manifestHash: manifest.manifestHash,
      manifestJson: JSON.stringify(manifest),
      expectedUnits: manifest.totalCoverageUnits,
      shards: plans.map(plan => ({
        id: plan.shardId,
        ordinal: plan.ordinal,
        unitIds: plan.primaryUnitIds,
        contextBeforeUnitIds: plan.contextBeforeUnitIds,
        contextAfterUnitIds: plan.contextAfterUnitIds,
      })),
    });
    store.markCoverageRunRunning({ runId: created.run.id });
    store.markCoverageShardRunning({ shardId: plans[0].shardId });
    store.completeCoverageShard({
      shardId: plans[0].shardId,
      resultJson: JSON.stringify({
        shardId: plans[0].shardId,
        processedUnitIds: plans[0].primaryUnitIds,
        findings: [],
        contradictions: [],
        openQuestions: [],
        warnings: [],
      } satisfies ShardResult),
    });
    store.markCoverageShardRunning({ shardId: plans[1].shardId }); // 崩溃残留 running

    const resumed = store.loadResumableCoverageRun({ manifestHash: manifest.manifestHash });
    expect(resumed!.run.id).toBe(created.run.id);
    expect(resumed!.shards.find(shard => shard.id === plans[1].shardId)!.status).toBe("pending");
    expect(resumed!.shards.find(shard => shard.id === plans[0].shardId)!.status).toBe("completed");

    const worker = vi.fn(async ({ prompt }: { prompt: string }) => validResultForPrompt(prompt));
    const result = await runCoverage({
      store, manifest, workerModel: worker as unknown as CoverageWorkerModel,
      concurrency: 2, shardTokenBudget: 300,
    });
    // completed shard 复用：worker 不再为 plans[0] 调用；其余（含置回 pending 的
    // plans[1]）续跑，最终 run complete。
    const executed = worker.mock.calls.map(call => shardIdOfPrompt((call[0] as any).prompt));
    expect(executed).not.toContain(plans[0].shardId);
    expect(executed).toContain(plans[1].shardId);
    expect(result.runId).toBe(created.run.id);
    expect(result.runStatus).toBe("complete");
    expect(result.ledger.processedPrimaryUnits).toBe(30);
    // 复用的结果原样出现在输出（persisted result_json 路径）。
    expect(result.shardResults.find(shard => shard.shardId === plans[0].shardId)).toBeDefined();

    // 终态 run 不再参与恢复；complete 的 run 不可再置 running。
    expect(store.loadResumableCoverageRun({ manifestHash: manifest.manifestHash })).toBeNull();
    expect(() => store.markCoverageRunRunning({ runId: created.run.id })).toThrow(/not resumable/);
  });

  it("取消语义：pending→cancelled、running 中止、completed 保留（§八十六）", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 30);
    const plans = planCoverageShards({ manifest, shardTokenBudget: 300 });
    const controller = new AbortController();
    const worker: CoverageWorkerModel = async ({ prompt }) => {
      if (shardIdOfPrompt(prompt) === plans[0].shardId) {
        return validResultForPrompt(prompt);
      }
      await new Promise(() => {}); // 永不返回：模拟在途 worker 被中止
    };
    const result = await runCoverage({
      store, manifest, workerModel: worker, concurrency: 1, shardTokenBudget: 300, signal: controller.signal,
      onProgress: (done) => {
        if (done === 1) controller.abort(); // 首个 shard 完成落库后取消
      },
    });
    expect(result.cancelled).toBe(true);
    expect(result.runStatus).toBe("cancelled");
    expect(result.gate.coverageStatus).toBe("partial");
    expect(result.gate.allowedClaim).toBe("partial_only");
    expect(result.ledger.skippedPrimaryUnits).toBe(30 - plans[0].primaryUnitIds.length);
    expect(result.ledger.processedPrimaryUnits).toBe(plans[0].primaryUnitIds.length);
    // completed 结果保留诊断。
    expect(result.shardResults.map(shard => shard.shardId)).toEqual([plans[0].shardId]);
    const state = store.getCoverageRun({ runId: result.runId })!;
    expect(state.shards.find(shard => shard.id === plans[0].shardId)!.status).toBe("completed");
    expect(state.shards.filter(shard => shard.status === "cancelled").length).toBe(plans.length - 1);
  });

  it("prompt 契约：含原始问题、plan 摘要、context/primary 标注与 unit 原文（§五十二）", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 6);
    const worker = okWorker();
    await runCoverage({
      store, manifest, workerModel: worker,
      question: "全书哪些章节提到灵犀协议？", shardTokenBudget: 300,
    });
    const prompt = worker.prompts[0];
    expect(prompt).toContain("Question: 全书哪些章节提到灵犀协议？");
    expect(prompt).toContain("intent=whole_scope_analysis");
    expect(prompt).toContain("coverageMode=exhaustive");
    expect(prompt).toContain("subQueries=全部要点 | 有无矛盾");
    expect(prompt).toContain("Primary units (scan EVERY one of them):");
    expect(prompt).toContain("unitId=cu_");
    expect(prompt).toContain("text:");
    expect(prompt).toContain("覆盖测试文本。");
    // 多 shard 下中间 shard 同时携带 before/after context 标注。
    const withContext = worker.prompts.find(p => p.includes("Context before (continuity only"));
    expect(withContext).toBeDefined();
    expect(withContext!).toContain("Context after (continuity only");
  });

  it("fidelity 场景：3 ready + 1 needs_ocr → text coverage 完整但绝不返回 full original coverage（§一百零一）", async () => {
    const store = createStore();
    const notebookA = notebook(store, "A");
    seedSource(store, [notebookA], { blockCount: 2 });
    seedSource(store, [notebookA], { blockCount: 2 });
    seedSource(store, [notebookA], { blockCount: 2 });
    seedSource(store, [notebookA], { blockCount: 1, status: "needs_ocr" });
    const manifest = buildCoverageManifest({
      source: store, studioId: STUDIO, scopeId: scopeOf(store, [notebookA]),
    });
    expect(manifest.sourceFidelitySummary).toMatchObject({ citation_grade: 3, needs_ocr: 1 });

    const result = await runCoverage({ store, manifest, workerModel: okWorker() });
    expect(result.gate.coverageStatus).toBe("complete");
    expect(result.gate.textCoverageRatio).toBe(1);
    expect(result.gate.sourceFidelitySummary.needs_ocr).toBe(1);
    expect(fidelityAllowsOriginalCoverageClaim(result.gate.sourceFidelitySummary)).toBe(false);
    expect(result.ledger.unavailableSources).toEqual([
      { sourceId: expect.any(String), fidelity: "needs_ocr" },
    ]);
    expect(fidelityAllowsOriginalCoverageClaim({
      citation_grade: 3, structural: 0, semantic_only: 0, needs_ocr: 0, unavailable: 0,
    })).toBe(true);
  });
});

// ───────────── Ledger / Gate / Evidence / 契约校验纯函数（§五十五/§五十六/§八十八） ─────────────

describe("Ledger / Gate / Evidence", () => {
  it("ledger 按 shard 终态记账：completed/failed/cancelled 分桶 + per-source 明细（§五十五）", () => {
    const store = createStore();
    const notebookA = notebook(store, "A");
    seedSource(store, [notebookA], { blockCount: 4 });
    const manifest = buildCoverageManifest({
      source: store, studioId: STUDIO, scopeId: scopeOf(store, [notebookA]),
    });
    const units = manifest.sources[0].coverageUnits.map(unit => unit.id);
    const ledger = computeCoverageLedger({
      manifest,
      shardStates: [
        { unitIds: [units[0], units[1]], status: "completed" },
        { unitIds: [units[2]], status: "failed" },
        { unitIds: [units[3]], status: "cancelled" },
        { unitIds: [], status: "pending" },
      ],
    });
    expect(ledger).toMatchObject({
      expectedPrimaryUnits: 4,
      processedPrimaryUnits: 2,
      failedPrimaryUnits: 1,
      skippedPrimaryUnits: 1,
    });
    expect(ledger.perSource[0]).toMatchObject({
      sourceId: manifest.sources[0].sourceId,
      expectedUnits: 4,
      processedUnits: 2,
      failedUnits: 1,
      skippedUnits: 1,
    });
    const gate = evaluateCoverageGate(ledger);
    expect(gate.coverageStatus).toBe("partial");
    expect(gate.allowedClaim).toBe("partial_only");
    expect(gate.textCoverageRatio).toBe(0.5);
  });

  it("expected=0（无可处理源）：gate partial、ratio 0，禁止虚标 complete（宁漏勿假）", () => {
    const store = createStore();
    const notebookA = notebook(store, "A");
    seedSource(store, [notebookA], { blockCount: 1, status: "needs_ocr" });
    const manifest = buildCoverageManifest({
      source: store, studioId: STUDIO, scopeId: scopeOf(store, [notebookA]),
    });
    const gate = evaluateCoverageGate(computeCoverageLedger({ manifest, shardStates: [] }));
    expect(gate.coverageStatus).toBe("partial");
    expect(gate.textCoverageRatio).toBe(0);
    expect(gate.allowedClaim).toBe("partial_only");
  });

  it("证据去重：同 statement 归一化合并、保留多独立 support；字符串列表去重（§八十八）", () => {
    const supportA = {
      sourceId: "src_1", snapshotId: "snap_1", parseArtifactId: "parse_1",
      blockId: "block_1", startOffset: 0, endOffset: 10,
    };
    const supportB = {
      sourceId: "src_1", snapshotId: "snap_1", parseArtifactId: "parse_1",
      blockId: "block_2", startOffset: 4, endOffset: 20,
    };
    const shardResults: ShardResult[] = [
      {
        shardId: "cshard_a",
        processedUnitIds: ["cu_a"],
        findings: [
          { statement: "灵犀协议支持全文覆盖。", support: [supportA] },
          { statement: "灵犀协议支持全文覆盖", support: [supportB] }, // 归一化后同 statement
        ],
        contradictions: ["章节 2 与章节 5 结论冲突"],
        openQuestions: ["未说明超时行为"],
        warnings: ["block_1 疑似乱码"],
      },
      {
        shardId: "cshard_b",
        processedUnitIds: ["cu_b"],
        findings: [
          { statement: "灵犀协议支持全文覆盖。", support: [supportA, supportB] },
        ],
        contradictions: ["章节 2 与章节 5 结论冲突", "章节 2 与章节 5 结论冲突"],
        openQuestions: [],
        warnings: [],
      },
    ];
    const evidence = aggregateShardEvidence(shardResults);
    expect(evidence.findings).toHaveLength(1);
    expect(evidence.findings[0].support).toHaveLength(2); // 两个独立区间都保留
    expect(evidence.contradictions).toEqual(["章节 2 与章节 5 结论冲突"]);
    expect(evidence.openQuestions).toEqual(["未说明超时行为"]);
    expect(evidence.warnings).toEqual(["block_1 疑似乱码"]);
  });

  it("parseShardResult：伪造 provenance / 未知 block / 重复 unit 列记 / 多余字段均非法", () => {
    const store = createStore();
    const notebookA = notebook(store, "A");
    const seeded = seedSource(store, [notebookA], { blockCount: 1 });
    const blocks = store.listArtifactBlocks({ studioId: STUDIO, parseArtifactId: seeded.artifactId });
    const manifest = buildCoverageManifest({
      source: store, studioId: STUDIO, scopeId: scopeOf(store, [notebookA]),
    });
    const units = manifest.sources[0].coverageUnits;
    expect(units).toHaveLength(1);
    expect(units[0].blockId).toBe(blocks[0].id);
    const unitsById = new Map(units.map(unit => [unit.id, unit]));
    const snapshotIdsBySource = new Map([[seeded.sourceId, seeded.snapshotId]]);
    const plan: CoverageShardPlan = planCoverageShards({ manifest })[0];
    const known = shardKnownBlocks({ shard: plan, unitsById, snapshotIdsBySource });
    const base = {
      shardId: plan.shardId,
      processedUnitIds: plan.primaryUnitIds,
      findings: [{
        statement: "s",
        support: [{
          sourceId: seeded.sourceId,
          snapshotId: seeded.snapshotId,
          parseArtifactId: seeded.artifactId,
          blockId: units[0].blockId,
          startOffset: 0,
          endOffset: 5,
        }],
      }],
      contradictions: [],
      openQuestions: [],
      warnings: [],
    };
    expect(parseShardResult({
      raw: JSON.stringify(base), shardId: plan.shardId,
      primaryUnitIds: plan.primaryUnitIds, knownBlocks: known,
    }).findings).toHaveLength(1);

    const forged = structuredClone(base);
    forged.findings[0].support[0].blockId = "block_unknown";
    expect(() => parseShardResult({
      raw: JSON.stringify(forged), shardId: plan.shardId,
      primaryUnitIds: plan.primaryUnitIds, knownBlocks: known,
    })).toThrow(/outside this shard/);

    const mismatched = structuredClone(base);
    mismatched.findings[0].support[0].snapshotId = "snap_other";
    expect(() => parseShardResult({
      raw: JSON.stringify(mismatched), shardId: plan.shardId,
      primaryUnitIds: plan.primaryUnitIds, knownBlocks: known,
    })).toThrow(/does not match the frozen block identity/);

    const duplicated = structuredClone(base);
    duplicated.processedUnitIds = [...plan.primaryUnitIds, ...plan.primaryUnitIds];
    expect(() => parseShardResult({
      raw: JSON.stringify(duplicated), shardId: plan.shardId,
      primaryUnitIds: plan.primaryUnitIds, knownBlocks: known,
    })).toThrow(/processedUnitIds/);

    const extra = { ...structuredClone(base), reasoning: "chain of thought" };
    expect(() => parseShardResult({
      raw: JSON.stringify(extra), shardId: plan.shardId,
      primaryUnitIds: plan.primaryUnitIds, knownBlocks: known,
    })).toThrow(/'reasoning' is invalid/);

    expect(() => parseShardResult({
      raw: "not json", shardId: plan.shardId,
      primaryUnitIds: plan.primaryUnitIds, knownBlocks: known,
    })).toThrow(/not valid JSON/);
  });

  it("buildShardWorkerPrompt：context unit 是合法 support 来源（containment 含 context）", () => {
    const store = createStore();
    const notebookA = notebook(store, "A");
    const seeded = seedSource(store, [notebookA], { blockCount: 6, blockText: BIG_BLOCK });
    const manifest = buildCoverageManifest({
      source: store, studioId: STUDIO, scopeId: scopeOf(store, [notebookA]),
    });
    const unitsById = new Map(manifest.sources[0].coverageUnits.map(unit => [unit.id, unit]));
    const snapshotIdsBySource = new Map([[seeded.sourceId, seeded.snapshotId]]);
    const plans = planCoverageShards({ manifest, shardTokenBudget: 300 });
    expect(plans.length).toBeGreaterThan(1);
    const second = plans[1];
    const known = shardKnownBlocks({ shard: second, unitsById, snapshotIdsBySource });
    for (const unitId of second.contextBeforeUnitIds) {
      expect(known.has(unitsById.get(unitId)!.blockId)).toBe(true);
    }
    const prompt = buildShardWorkerPrompt({
      question: "q",
      planSummary: PLAN_SUMMARY,
      shard: second,
      unitsById,
      snapshotIdsBySource,
    });
    expect(prompt).toContain("Context before (continuity only");
    expect(validResultForPrompt(prompt)).toContain(second.shardId);
  });
});

// ─────────────── 开销感知装填 / 熔断（2026-08-30 延迟加固） ───────────────

describe("分片预算含渲染开销（行级小单元源）", () => {
  it("300 个行级小 block：每片渲染后 prompt 仍在预算口径内（正文+头 ≤ shardTokenBudget）", () => {
    const store = createStore();
    const nb = notebook(store, "A");
    // 行级源（XLSX/CSV 一行一 block 的形状，§五十九）：正文 ~11 token/行，
    // provenance 头（unitId sha256/snapshot/parseArtifact/blockId/offsets）反而
    // 是大头——只按正文装填时 300 行会挤进同一片，渲染后 prompt 3 倍+ 超预算
    // （实测 54k token/片 → 线性化超时全灭）。
    const seeded = seedSource(store, [nb], {
      blockCount: 300,
      blockText: (index) => `行${index}:单价${index}数量${index}备注行数据。`,
    });
    const manifest = buildCoverageManifest({
      source: store,
      studioId: STUDIO,
      scopeId: scopeOf(store, [nb]),
    });
    const unitsById = new Map(manifest.sources.flatMap(source =>
      source.coverageUnits.map(unit => [unit.id, unit] as const)));
    const snapshotIdsBySource = new Map(manifest.sources.map(source =>
      [source.sourceId, source.contentSnapshotId]));
    const plans = planCoverageShards({ manifest });
    // 装填确实在切：小单元源不应一片装完。
    expect(plans.length).toBeGreaterThan(1);
    // 渲染后不变式：每片 prompt 估算 ≤ 预算 + context 窗口（±2 单元）+ 头部
    // 固定行（question/plan/schema 指令）。旧口径下这里会到 ~54k。
    for (const plan of plans) {
      const prompt = buildShardWorkerPrompt({
        question: "请完整梳理全部资料要点，不要遗漏",
        planSummary: PLAN_SUMMARY,
        shard: plan,
        unitsById,
        snapshotIdsBySource,
      });
      expect(estimateTextTokens(prompt)).toBeLessThanOrEqual(
        COVERAGE_SHARD_TOKEN_BUDGET + 2 * COVERAGE_UNIT_TOKEN_BUDGET + 500,
      );
    }
    // 全量 unit 仍然恰好各属一片 primary（exhaustive 语义不受装填口径影响）。
    const primaryCount = new Map<string, number>();
    for (const plan of plans) {
      for (const unitId of plan.primaryUnitIds) primaryCount.set(unitId, (primaryCount.get(unitId) ?? 0) + 1);
    }
    expect(primaryCount.size).toBe(300);
    for (const unit of manifestUnitSequence(manifest)) {
      expect(primaryCount.get(unit.id)).toBe(1);
    }
    expect(seeded.sourceId).toBeTruthy();
  });
});

describe("Coverage 熔断（零成功 + 终态失败达阈值）", () => {
  it("worker 持续失败：第 4 个终态 failed shard 后提前取消剩余，reasonCode=CIRCUIT_BREAK", async () => {
    const store = createStore();
    // 400 个 BIG_BLOCK units ≈ 9 shards（开销感知口径）：> 阈值 4。
    const manifest = manifestWithUnits(store, 400);
    const plans = planCoverageShards({ manifest });
    expect(plans.length).toBeGreaterThan(COVERAGE_CIRCUIT_BREAK_FAILURES);
    let calls = 0;
    const worker: CoverageWorkerModel = async () => {
      calls += 1;
      return "{invalid";
    };
    const result = await runCoverage({ store, manifest, workerModel: worker });

    expect(result.reasonCode).toBe(KNOWLEDGE_COVERAGE_CIRCUIT_BREAK);
    expect(result.cancelled).toBe(true);
    // 提前终止：只有第一个波次（4 shards）烧完 bounded retry，未开跑的直接取消。
    const state = store.getCoverageRun({ runId: result.runId })!;
    const failed = state.shards.filter(shard => shard.status === "failed");
    const cancelledShards = state.shards.filter(shard => shard.status === "cancelled");
    expect(failed).toHaveLength(COVERAGE_CIRCUIT_BREAK_FAILURES);
    expect(cancelledShards.length).toBe(plans.length - COVERAGE_CIRCUIT_BREAK_FAILURES);
    expect(calls).toBe(COVERAGE_CIRCUIT_BREAK_FAILURES * COVERAGE_SHARD_MAX_ATTEMPTS * 2);
    // ledger 如实：零成功、失败单元计入、绝不 complete。
    expect(result.ledger.processedPrimaryUnits).toBe(0);
    expect(result.ledger.failedPrimaryUnits).toBe(
      failed.reduce((sum, shard) => sum + shard.unitIds.length, 0),
    );
    expect(result.gate.coverageStatus).not.toBe("complete");
  });

  it("任一 shard 成功即豁免熔断：全灭之外的失败仍走 PARTIAL 留痕", async () => {
    const store = createStore();
    const manifest = manifestWithUnits(store, 400);
    const plans = planCoverageShards({ manifest });
    let succeeded = 0;
    const worker: CoverageWorkerModel = async ({ prompt }) => {
      // 只有第一片成功，其余全部失败——终态失败数远超阈值但有成功 → 不熔断。
      if (/ordinal 0\)/.test(prompt)) {
        succeeded += 1;
        return validResultForPrompt(prompt);
      }
      return "{invalid";
    };
    const result = await runCoverage({ store, manifest, workerModel: worker });

    expect(succeeded).toBe(1);
    expect(result.reasonCode).toBe(KNOWLEDGE_COVERAGE_PARTIAL);
    expect(result.cancelled).toBe(false);
    const state = store.getCoverageRun({ runId: result.runId })!;
    // 没有任何 shard 被 circuit 取消：失败片全部走满 bounded retry 终态 failed。
    expect(state.shards.filter(shard => shard.status === "cancelled")).toHaveLength(0);
    expect(state.shards.filter(shard => shard.status === "failed")).toHaveLength(plans.length - 1);
    expect(state.shards.filter(shard => shard.status === "completed")).toHaveLength(1);
  });
});
