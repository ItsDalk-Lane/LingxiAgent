/**
 * Knowledge Coverage 层级归约（任务书 §六十–§六十二/§一百零三，Phase 10）：
 * - 层级管道：Shard Evidence（稳定 ev_ id）→ Source → Notebook（共享源复用）→
 *   Cross-Notebook（合并去重）；每级预算内原样传递、超预算调 reduceModel；
 * - 防失真（§六十二）：support 全集守恒（禁伪造禁丢弃）、notes verbatim、
 *   id 必须是输入 id 或升序 '+' 拼接；违例纠错一次再失败 → 该级失败降级截断留痕；
 * - §一百零三：大语料总 token 远超注入预算 → 按层级 Shard→Reduce，最终注入
 *   有界，evidence id 链可从注入块回溯到 shard provenance；
 * - 降级矩阵：reduceModel 未配 / 调用失败 / 输出两次非法 → 保序结构化截断
 *   + degradedReason 留痕；并发有界（共享并发上限常量）。
 * 纯函数路径：ShardResult[]/manifest 直接构造，无 store、无 IO。
 */
import { describe, expect, it } from "vitest";

import {
  assembleShardEvidenceObjects,
  buildCoverageReductionPrompt,
  estimateEvidenceTokens,
  parseReducedEvidence,
  reduceCoverageEvidence,
  KNOWLEDGE_COVERAGE_REDUCTION_CONCURRENCY,
  type CoverageEvidenceSet,
  type CoverageReduceModel,
} from "../lib/knowledge/knowledge-coverage-reduction.ts";
import type {
  CoverageManifest,
  ShardFindingSupport,
  ShardResult,
} from "../lib/knowledge/knowledge-coverage-manifest.ts";

const QUESTION = "请完整梳理全部资料要点，不要遗漏";

function supportOf(sourceId: string, index = 0): ShardFindingSupport {
  return {
    sourceId,
    snapshotId: `snap-${sourceId}`,
    parseArtifactId: `art-${sourceId}`,
    blockId: `blk-${sourceId}-${index}`,
    startOffset: 0,
    endOffset: 100,
  };
}

function shardResultOf(
  shardId: string,
  findings: Array<{ statement: string; sourceId: string; supportIndex?: number }>,
  extras: Partial<Pick<ShardResult, "contradictions" | "openQuestions" | "warnings">> = {},
): ShardResult {
  return {
    shardId,
    processedUnitIds: [],
    findings: findings.map(finding => ({
      statement: finding.statement,
      support: [supportOf(finding.sourceId, finding.supportIndex ?? 0)],
    })),
    contradictions: extras.contradictions ?? [],
    openQuestions: extras.openQuestions ?? [],
    warnings: extras.warnings ?? [],
  };
}

function manifestOf(sources: Array<{ sourceId: string; memberships: string[] }>): CoverageManifest {
  return {
    coverageRunId: null,
    turnScopeId: "ts-reduction-test",
    sources: sources.map(source => ({
      sourceId: source.sourceId,
      contentSnapshotId: `snap-${source.sourceId}`,
      parseArtifactId: `art-${source.sourceId}`,
      notebookMemberships: source.memberships,
      fidelity: "citation_grade" as const,
      coverageUnits: [],
    })),
    totalSources: sources.length,
    totalCoverageUnits: 0,
    sourceFidelitySummary: { citation_grade: sources.length, structural: 0, semantic_only: 0, needs_ocr: 0, unavailable: 0 },
    createdAt: "2026-08-29T00:00:00.000Z",
    manifestHash: "manifest-hash-reduction-test",
  };
}

/** 从归约 prompt 提取输入证据集（prompt 的 JSON 单行呈现契约）。 */
function inputEvidenceOfPrompt(prompt: string): CoverageEvidenceSet {
  const marker = "Input evidence (JSON";
  const markerIndex = prompt.indexOf(marker);
  expect(markerIndex).toBeGreaterThanOrEqual(0);
  const jsonStart = prompt.indexOf("{", markerIndex);
  const jsonEnd = prompt.indexOf("\n", jsonStart);
  return JSON.parse(prompt.slice(jsonStart, jsonEnd)) as CoverageEvidenceSet;
}

/** 合法归约输出：全量合并为一条 finding（id 升序拼接 + support 并集 + notes verbatim）。 */
function mergeAllOutput(input: CoverageEvidenceSet): string {
  const ids = input.findings.map(finding => finding.id).sort();
  const supports = new Map<string, ShardFindingSupport>();
  for (const finding of input.findings) {
    for (const support of finding.support) supports.set(JSON.stringify(support), support);
  }
  return JSON.stringify({
    findings: [{
      id: ids.join("+"),
      statement: `合并陈述（${ids.length} 条）：${input.findings[0]?.statement.slice(0, 160) ?? ""}`,
      support: [...supports.values()],
    }],
    contradictions: input.contradictions,
    openQuestions: input.openQuestions,
    warnings: input.warnings,
  });
}

interface ReduceCall {
  level: string;
  group: string;
  correction: boolean;
}

function trackingReducer(
  calls: ReduceCall[],
  impl: CoverageReduceModel = async ({ prompt }) => mergeAllOutput(inputEvidenceOfPrompt(prompt)),
): CoverageReduceModel {
  return async (input) => {
    const match = /Level: (\S+) evidence reduction \(group ([^)]+)\)/.exec(input.prompt);
    calls.push({ level: match?.[1] ?? "?", group: match?.[2] ?? "?", correction: input.correction != null });
    return impl(input);
  };
}

const range = (length: number) => Array.from({ length }, (_, index) => index);

/** ~500 tokens/条的大陈述（降级/纠错/并发用例）。 */
const BIG_STATEMENT = (sourceId: string, index: number) =>
  `${sourceId} 发现 ${index}：` + "大语料证据陈述，供层级归约压缩。".repeat(30);

/** ~2400 tokens/条的巨陈述（§一百零三 规模：statement 主导，合并后节省 >60%）。 */
const HUGE_STATEMENT = (sourceId: string, index: number) =>
  `${sourceId} 发现 ${index}：` + "长语料证据陈述，供层级归约压缩验证。".repeat(120);

// ── Shard Evidence 装配与稳定 id（§六十一） ─────────────────────────────

describe("Shard Evidence 装配", () => {
  it("跨 shard 重复发现按 statement 归一合并：support 并集保留、id 稳定（ev_ + 16 hex）、notes 去重", () => {
    const set = assembleShardEvidenceObjects([
      shardResultOf("cshard_a", [{ statement: "事实甲。", sourceId: "src-1" }]),
      shardResultOf("cshard_b", [
        { statement: "事实甲。", sourceId: "src-1", supportIndex: 1 },
        { statement: "事实乙。", sourceId: "src-2" },
      ], { contradictions: ["矛盾一"], openQuestions: ["问题一"], warnings: ["警告一"] }),
      shardResultOf("cshard_c", [{ statement: "事实甲 ", sourceId: "src-2" }], { contradictions: ["矛盾一"] }),
    ]);

    expect(set.findings).toHaveLength(2);
    const merged = set.findings.find(finding => finding.statement === "事实甲。")!;
    expect(merged.id).toMatch(/^ev_[0-9a-f]{16}$/);
    // src-1 的两个锚点 + src-2 的锚点：多独立 support 不丢（§八十八）。
    expect(merged.support).toHaveLength(3);
    expect(set.contradictions).toEqual(["矛盾一"]);
    expect(set.openQuestions).toEqual(["问题一"]);
    expect(set.warnings).toEqual(["警告一"]);

    // 同输入重放 → 同 id（稳定，可持久化追踪）。
    const replay = assembleShardEvidenceObjects([
      shardResultOf("cshard_a", [{ statement: "事实甲。", sourceId: "src-1" }]),
    ]);
    expect(replay.findings[0].id).toMatch(/^ev_[0-9a-f]{16}$/);
    expect(replay.findings[0].id).not.toBe(merged.id);
  });
});

// ── 层级管道（§六十一/§一百零三） ───────────────────────────────────────

describe("层级归约管道", () => {
  it("小语料：预算内零 LLM 归约调用，证据对象原样传递", async () => {
    const manifest = manifestOf([{ sourceId: "src-1", memberships: ["nb-1"] }]);
    const shardResults = [
      shardResultOf("cshard_a", [{ statement: "小语料事实。", sourceId: "src-1" }]),
    ];
    const outcome = await reduceCoverageEvidence({
      shardResults,
      manifest,
      question: QUESTION,
      injectionBudgetTokens: 6000,
      reduceModel: async () => {
        throw new Error("reduceModel must not be called within budget");
      },
    });

    expect(outcome.degradedReason).toBeNull();
    expect(outcome.truncated).toBe(false);
    expect(outcome.levels).toHaveLength(3);
    expect(outcome.levels.every(level => level.reduced === false)).toBe(true);
    expect(outcome.levels.map(level => level.level)).toEqual(["source", "notebook", "cross_notebook"]);
    expect(outcome.evidence.findings).toHaveLength(1);
    expect(outcome.evidence.findings[0].id).toMatch(/^ev_[0-9a-f]{16}$/);
    expect(outcome.shardEvidenceCount).toBe(1);
  });

  it("§一百零三大语料：source/notebook/cross 级都压缩，最终注入 token 有界，id 链与 support 均可回溯到 worker 输出", async () => {
    const manifest = manifestOf([
      { sourceId: "src-1", memberships: ["nb-a"] },
      { sourceId: "src-2", memberships: ["nb-a", "nb-b"] }, // 共享源：两组复用
      { sourceId: "src-3", memberships: ["nb-b"] },
    ]);
    const shardResults = [
      shardResultOf("cshard_0", range(3).map(index => ({ statement: HUGE_STATEMENT("src-1", index), sourceId: "src-1", supportIndex: index }))),
      shardResultOf("cshard_1", range(3).map(index => ({ statement: HUGE_STATEMENT("src-2", index), sourceId: "src-2", supportIndex: index }))),
      shardResultOf("cshard_2", range(3).map(index => ({ statement: HUGE_STATEMENT("src-3", index), sourceId: "src-3", supportIndex: index }))),
    ];
    const shardSet = assembleShardEvidenceObjects(shardResults);
    const shardTotal = estimateEvidenceTokens(shardSet.findings);
    // 总量远超注入预算（~22k tokens vs 900），且每源 ~7k 也远超 source 级预算。
    expect(shardTotal).toBeGreaterThan(10_000);
    const sourceGroupEstimate = estimateEvidenceTokens(
      shardSet.findings.filter(finding => finding.support.some(support => support.sourceId === "src-1")));
    expect(sourceGroupEstimate).toBeGreaterThan(5000);

    const calls: ReduceCall[] = [];
    const outcome = await reduceCoverageEvidence({
      shardResults,
      manifest,
      question: QUESTION,
      injectionBudgetTokens: 900,
      // source 级按实测相对值设定（输入的 20%）：必然触发压缩，合并输出（≈6%）必过。
      sourceBudgetTokens: Math.floor(sourceGroupEstimate * 0.2),
      notebookBudgetTokens: 650,
      reduceModel: trackingReducer(calls),
    });

    // 三个层级都发生了压缩（按层级 source → notebook → cross_notebook）。
    expect(outcome.levels.map(level => [level.level, level.reduced])).toEqual([
      ["source", true],
      ["notebook", true],
      ["cross_notebook", true],
    ]);
    expect(new Set(calls.map(call => call.level))).toEqual(new Set(["source", "notebook", "cross_notebook"]));
    // 共享源 → 两个 notebook 组都归约。
    expect(calls.filter(call => call.level === "notebook")).toHaveLength(2);
    expect(outcome.groupCounts).toEqual({ source: 3, notebook: 2 });

    // 最终注入有界（§一百零三：不能全部一次性进主模型）。
    const finalEstimate = estimateEvidenceTokens(outcome.evidence.findings);
    expect(finalEstimate).toBeLessThanOrEqual(900);
    expect(finalEstimate).toBeLessThan(shardTotal / 10);
    expect(outcome.degradedReason).toBeNull();
    expect(outcome.truncated).toBe(false);

    // evidence id 链回溯：最终 id 的每一段都在 shard 级证据 id 集合内。
    const inputIds = new Set(shardSet.findings.map(finding => finding.id));
    const workerSupports = new Set(shardResults.flatMap(result =>
      result.findings.map(finding => JSON.stringify(finding.support[0]))));
    const finalIds = new Set<string>();
    for (const finding of outcome.evidence.findings) {
      for (const part of finding.id.split("+")) {
        expect(inputIds.has(part)).toBe(true);
        finalIds.add(part);
      }
      // support 全部来自 worker 输出的 provenance（禁伪造）。
      for (const support of finding.support) {
        expect(workerSupports.has(JSON.stringify(support))).toBe(true);
      }
    }
    // 合并确实收敛：最终对象数 < shard 级对象数。
    expect(outcome.evidence.findings.length).toBeLessThan(shardSet.findings.length);
    expect(finalIds.size).toBeGreaterThan(0);
    // cross 级合并去重：无重复 id。
    const allIds = outcome.evidence.findings.map(finding => finding.id);
    expect(new Set(allIds).size).toBe(allIds.length);
  });

  it("纠错重试：首次输出非法（id 乱序拼接）→ correction 重发 → 合法输出被采纳", async () => {
    const manifest = manifestOf([{ sourceId: "src-1", memberships: ["nb-1"] }]);
    const shardResults = [
      shardResultOf("cshard_0", range(4).map(index => ({ statement: BIG_STATEMENT("src-1", index), sourceId: "src-1", supportIndex: index }))),
    ];
    let attempt = 0;
    const calls: ReduceCall[] = [];
    const reduceModel: CoverageReduceModel = async (input) => {
      attempt += 1;
      calls.push({ level: "source", group: "src-1", correction: input.correction != null });
      if (attempt === 1) {
        const parsed = inputEvidenceOfPrompt(input.prompt);
        const ids = parsed.findings.map(finding => finding.id).sort().reverse();
        return JSON.stringify({
          findings: [{ id: ids.join("+"), statement: "合并陈述", support: parsed.findings.flatMap(f => f.support) }],
          contradictions: parsed.contradictions,
          openQuestions: parsed.openQuestions,
          warnings: parsed.warnings,
        });
      }
      expect(input.correction).toBeDefined();
      expect(input.correction!.error).toContain("ascending order");
      const parsed = inputEvidenceOfPrompt(input.prompt);
      return mergeAllOutput(parsed);
    };
    const outcome = await reduceCoverageEvidence({
      shardResults,
      manifest,
      question: QUESTION,
      injectionBudgetTokens: 6000,
      sourceBudgetTokens: 500,
      reduceModel,
    });

    expect(outcome.degradedReason).toBeNull();
    expect(outcome.levels[0].reduced).toBe(true);
    expect(attempt).toBe(2);
    expect(calls.filter(call => call.correction)).toHaveLength(1);
  });
});

// ── 防失真校验（§六十二） ───────────────────────────────────────────────

describe("防失真校验", () => {
  const baseSet = (): CoverageEvidenceSet => ({
    findings: [
      { id: "ev_aaaaaaaaaaaaaaaa", statement: "正面事实。", support: [supportOf("src-1", 0)] },
      { id: "ev_bbbbbbbbbbbbbbbb", statement: "负面发现：未找到 X 的任何记录。", support: [supportOf("src-1", 1)] },
    ],
    contradictions: ["记录一与记录二冲突"],
    openQuestions: ["X 的数值是多少"],
    warnings: ["块 3 解析质量差"],
  });

  it("丢弃 finding（连 support）→ 拒绝；伪造 support → 拒绝；notes 被改写 → 拒绝", () => {
    const source = baseSet();
    // ① 丢 finding：只剩第一条，support 全集缺失。
    const dropped = JSON.stringify({
      findings: [source.findings[0]],
      contradictions: source.contradictions,
      openQuestions: source.openQuestions,
      warnings: source.warnings,
    });
    expect(() => parseReducedEvidence({ raw: dropped, source, outputBudgetTokens: 6000 }))
      .toThrow(/dropped support provenance/);

    // ② 伪造 support：blockId 不在输入集合。
    const fabricated = JSON.parse(dropped) as Record<string, unknown>;
    (fabricated.findings as Array<Record<string, unknown>>)[0] = {
      id: "ev_a",
      statement: "正面事实。",
      support: [{ ...supportOf("src-1", 0), blockId: "blk-forged" }],
    };
    expect(() => parseReducedEvidence({ raw: JSON.stringify(fabricated), source, outputBudgetTokens: 6000 }))
      .toThrow(/not present in the input evidence/);

    // ③ notes 被改写（删减/措辞变化均非法；support 全集保持完整以隔离该校验）。
    const alteredNotes = JSON.parse(JSON.stringify({
      findings: source.findings,
      contradictions: source.contradictions,
      openQuestions: source.openQuestions,
      warnings: source.warnings,
    })) as Record<string, unknown>;
    alteredNotes.openQuestions = [];
    expect(() => parseReducedEvidence({ raw: JSON.stringify(alteredNotes), source, outputBudgetTokens: 6000 }))
      .toThrow(/must be preserved verbatim/);
  });

  it("输出超预算 → 拒绝（层级边界有界性的机器保证）", () => {
    const source = baseSet();
    const raw = JSON.stringify({
      findings: [{
        id: "ev_aaaaaaaaaaaaaaaa+ev_bbbbbbbbbbbbbbbb",
        statement: "合并陈述。" + "超长但未破字符上限的陈述。".repeat(280),
        support: source.findings.flatMap(finding => finding.support),
      }],
      contradictions: source.contradictions,
      openQuestions: source.openQuestions,
      warnings: source.warnings,
    });
    expect(() => parseReducedEvidence({ raw, source, outputBudgetTokens: 500 }))
      .toThrow(/exceeds the level token budget/);
  });

  it("归约模型持续丢弃证据：纠错一次仍失败 → 该级失败 → 降级结构化截断 + 留痕（不静默不 prose 化）", async () => {
    const manifest = manifestOf([{ sourceId: "src-1", memberships: ["nb-1"] }]);
    const shardResults = [
      shardResultOf("cshard_0", range(4).map(index => ({ statement: BIG_STATEMENT("src-1", index), sourceId: "src-1", supportIndex: index }))),
    ];
    const calls: ReduceCall[] = [];
    const droppingReducer: CoverageReduceModel = async (input) => {
      const parsed = inputEvidenceOfPrompt(input.prompt);
      calls.push({ level: "source", group: "src-1", correction: input.correction != null });
      // 持续只回第一条 finding：负面/其余证据连 support 一起丢。
      return JSON.stringify({
        findings: [parsed.findings[0]],
        contradictions: parsed.contradictions,
        openQuestions: parsed.openQuestions,
        warnings: parsed.warnings,
      });
    };
    const outcome = await reduceCoverageEvidence({
      shardResults,
      manifest,
      question: QUESTION,
      injectionBudgetTokens: 200,
      sourceBudgetTokens: 2000,
      reduceModel: droppingReducer,
    });

    expect(outcome.degradedReason).toMatch(/source level reduction failed/);
    expect(outcome.degradedReason).toMatch(/dropped support provenance/);
    expect(outcome.truncated).toBe(true);
    expect(outcome.omittedFindings).toBeGreaterThan(0);
    // 纠错重试纪律：同组两次调用，第二次带 correction。
    expect(calls).toHaveLength(2);
    expect(calls[1].correction).toBe(true);
    // 降级走保序截断：保留的是 shard 级证据的前缀，id 未被篡改。
    const shardSet = assembleShardEvidenceObjects(shardResults);
    expect(outcome.evidence.findings).toEqual(shardSet.findings.slice(0, outcome.evidence.findings.length));
  });

  it("归约模型伪造 support：同样纠错 → 再失败 → 降级留痕", async () => {
    const manifest = manifestOf([{ sourceId: "src-1", memberships: ["nb-1"] }]);
    const shardResults = [
      shardResultOf("cshard_0", range(2).map(index => ({ statement: BIG_STATEMENT("src-1", index), sourceId: "src-1", supportIndex: index }))),
    ];
    const fabricatedReducer: CoverageReduceModel = async (input) => {
      const parsed = inputEvidenceOfPrompt(input.prompt);
      return JSON.stringify({
        findings: [{
          id: parsed.findings.map(finding => finding.id).sort().join("+"),
          statement: "伪造 provenance 的合并陈述",
          support: [{ ...parsed.findings[0].support[0], blockId: "blk-not-in-shard" }],
        }],
        contradictions: parsed.contradictions,
        openQuestions: parsed.openQuestions,
        warnings: parsed.warnings,
      });
    };
    const outcome = await reduceCoverageEvidence({
      shardResults,
      manifest,
      question: QUESTION,
      injectionBudgetTokens: 6000,
      sourceBudgetTokens: 100,
      reduceModel: fabricatedReducer,
    });
    expect(outcome.degradedReason).toMatch(/not present in the input evidence/);
    expect(outcome.truncated).toBe(false); // 降级截断后预算内放得下，无遗漏
    expect(outcome.evidence.findings).toHaveLength(2);
  });
});

// ── 降级矩阵与并发（§六十二/泵模式） ──────────────────────────────────

describe("降级与并发", () => {
  it("reduceModel 未配：零 LLM 调用 + 保序结构化截断 + degradedReason 留痕", async () => {
    const manifest = manifestOf([{ sourceId: "src-1", memberships: ["nb-1"] }]);
    const shardResults = [
      shardResultOf("cshard_0", range(6).map(index => ({ statement: BIG_STATEMENT("src-1", index), sourceId: "src-1" }))),
    ];
    const outcome = await reduceCoverageEvidence({
      shardResults,
      manifest,
      question: QUESTION,
      injectionBudgetTokens: 400,
      reduceModel: null,
    });

    expect(outcome.degradedReason).toBe("coverage reduce model not configured");
    expect(outcome.truncated).toBe(true);
    expect(outcome.omittedFindings).toBeGreaterThan(0);
    expect(outcome.levels.every(level => level.reduced === false)).toBe(true);
    // 截断有界：保留部分不超过注入预算。
    expect(estimateEvidenceTokens(outcome.evidence.findings)).toBeLessThanOrEqual(400);
  });

  it("模型调用抛错（非输出问题）：该级失败降级留痕", async () => {
    const manifest = manifestOf([{ sourceId: "src-1", memberships: ["nb-1"] }]);
    const shardResults = [
      shardResultOf("cshard_0", range(4).map(index => ({ statement: BIG_STATEMENT("src-1", index), sourceId: "src-1" }))),
    ];
    const outcome = await reduceCoverageEvidence({
      shardResults,
      manifest,
      question: QUESTION,
      injectionBudgetTokens: 6000,
      sourceBudgetTokens: 100,
      reduceModel: async () => {
        throw new Error("provider unavailable");
      },
    });
    expect(outcome.degradedReason).toMatch(/source level reduction failed/);
    expect(outcome.degradedReason).toMatch(/provider unavailable/);
  });

  it("多组并行归约有界：峰值在途 ≤ 并发上限（共享常量默认 4）", async () => {
    const manifest = manifestOf(range(6).map(index => ({
      sourceId: `src-${index}`,
      memberships: [`nb-${index}`],
    })));
    const shardResults = range(6).map(index =>
      shardResultOf(`cshard_${index}`, range(4).map(inner => ({
        statement: BIG_STATEMENT(`src-${index}`, inner),
        sourceId: `src-${index}`,
        supportIndex: inner,
      }))));
    let inFlight = 0;
    let peak = 0;
    const slowReducer: CoverageReduceModel = async ({ prompt }) => {
      inFlight += 1;
      peak = Math.max(peak, inFlight);
      await new Promise(resolve => setTimeout(resolve, 5));
      inFlight -= 1;
      return mergeAllOutput(inputEvidenceOfPrompt(prompt));
    };
    const outcome = await reduceCoverageEvidence({
      shardResults,
      manifest,
      question: QUESTION,
      injectionBudgetTokens: 6000,
      sourceBudgetTokens: 700,
      notebookBudgetTokens: 6000,
      reduceModel: slowReducer,
    });
    expect(outcome.degradedReason).toBeNull();
    expect(peak).toBeLessThanOrEqual(KNOWLEDGE_COVERAGE_REDUCTION_CONCURRENCY);
    expect(peak).toBeGreaterThan(1); // 确实并行了（6 组 > 上限 4）
  });
});

// ── prompt 组装（防失真指令显式在场） ──────────────────────────────────

describe("归约 prompt", () => {
  it("包含防失真规则与结构化 schema（§六十二）：禁丢弃/禁伪造/verbatim notes/id 拼接纪律", () => {
    const prompt = buildCoverageReductionPrompt({
      question: QUESTION,
      level: "source",
      groupLabel: "src-1",
      evidence: {
        findings: [{ id: "ev_aaaaaaaaaaaaaaaa", statement: "事实。", support: [supportOf("src-1")] }],
        contradictions: [],
        openQuestions: [],
        warnings: [],
      },
      outputBudgetTokens: 1000,
    });
    expect(prompt).toContain("Never drop a finding");
    expect(prompt).toContain("no fabricated provenance, no omissions");
    expect(prompt).toContain("copied verbatim");
    expect(prompt).toContain('joined with "+" in ascending order');
    expect(prompt).toContain(`Question: ${QUESTION}`);
    expect(prompt).toContain("Level: source evidence reduction (group src-1)");
    expect(prompt).toContain("ev_aaaaaaaaaaaaaaaa");
  });
});
