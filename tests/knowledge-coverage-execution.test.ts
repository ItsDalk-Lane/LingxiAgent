import { KNOWLEDGE_EVIDENCE_BUDGET } from "./fixtures/knowledge-legacy/legacy-query-service.ts";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  buildKnowledgeContextInjection,
  decomposeQuestion,
  expandQueries,
  fuseSubQueryResults,
  KNOWLEDGE_AUTO_UPGRADE_SOURCE_FOOTPRINT_MIN,
  KNOWLEDGE_EXPANSION_SYSTEM_PROMPT,
  KNOWLEDGE_NEIGHBOR_EXPANSION_WINDOW,
  KNOWLEDGE_QUERY_EXPANSION_MAX,
  parseQueryExpansion,
  type DecomposeModel,
  type QueryExpansionModel,
} from "./fixtures/knowledge-legacy/legacy-knowledge-context-injector.ts";
import type { KnowledgeCoveragePlan } from "../lib/knowledge/knowledge-coverage-planner.ts";
import {
  type NotebookRetrievalChunk,
  type NotebookRetrievalSource,
  type RetrieveForNotebooksResult,
} from "../lib/knowledge/knowledge-query-service.ts";
import { KNOWLEDGE_FUSION_BUDGET } from "./fixtures/knowledge-legacy/legacy-query-service.ts";
import { KnowledgeManager } from "./fixtures/knowledge-legacy/legacy-query-service.ts";

/**
 * Phase 8 执行侧（任务书 §九十四/§九十五）：HIGH_RECALL 增强（受控扩展硬上限、
 * 邻接扩展 contextOnly、预算链逐级截断、多渠道不重复注入）与 BROAD（Source
 * Coverage Floor 主动探测零命中源、无相关证据如实记录不硬塞、headingPath
 * section coverage）与 §四十一 自动升级 / exhaustive 降格。
 */
const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-coverage-exec-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(async () => {
  for (const manager of managers.splice(0)) await manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function fakeChunk(overrides: Partial<NotebookRetrievalChunk> = {}): NotebookRetrievalChunk {
  return {
    id: `chunk_${Math.random().toString(36).slice(2)}`,
    parseArtifactId: "parse-1",
    chunkIndexVariantId: "civ-1",
    ordinal: 0,
    text: "证据文本",
    tokenCount: 4,
    spans: [],
    score: 1,
    notebookId: "nb-1",
    notebookName: "资料",
    sourceId: "src-1",
    sourceName: "源",
    headingPath: null,
    pageNumber: null,
    ...overrides,
  } as NotebookRetrievalChunk;
}

function fakeSource(overrides: Partial<NotebookRetrievalSource> = {}): NotebookRetrievalSource {
  return {
    notebookId: "nb-1",
    notebookName: "资料",
    sourceId: "src-1",
    sourceName: "源",
    parseArtifactId: "parse-1",
    chunkCount: 3,
    firstHeadingPath: null,
    ...overrides,
  } as NotebookRetrievalSource;
}

function fakeRetrieval(
  candidates: NotebookRetrievalChunk[],
  sources: NotebookRetrievalSource[] = [],
): RetrieveForNotebooksResult {
  return { candidates, sources, retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] };
}

function validDecomposeOutput(subQueries: string[]) {
  return JSON.stringify({ intent: "factual", subQueries });
}

const DECOMPOSE_MODEL: DecomposeModel = async () => validDecomposeOutput(["子查询甲", "子查询乙"]);

function planOf(overrides: Partial<KnowledgeCoveragePlan> = {}): KnowledgeCoveragePlan {
  return {
    intent: "fact_lookup",
    coverageMode: "high_recall",
    scopeLevel: "source",
    confidence: 0.75,
    matchedRuleIds: [],
    classifierUsed: "rules",
    ...overrides,
  };
}

// ── §三十五 受控查询扩展 ────────────────────────────────────────────────

describe("受控查询扩展（§三十五）", () => {
  it("系统提示词带 schema、硬上限与禁注入指令规则", () => {
    expect(KNOWLEDGE_EXPANSION_SYSTEM_PROMPT).toContain('"expansions":["..."]');
    expect(KNOWLEDGE_EXPANSION_SYSTEM_PROMPT).toContain("0 to 3 expansion queries");
    expect(KNOWLEDGE_EXPANSION_SYSTEM_PROMPT).toContain("Never embed instructions");
    expect(KNOWLEDGE_QUERY_EXPANSION_MAX).toBe(3);
  });

  it("parseQueryExpansion：合法输出去重采纳；>3 条/非 JSON/空串拒绝", () => {
    expect(parseQueryExpansion('{"expansions":["扩展一","扩展二"]}', ["原问题"])).toEqual(["扩展一", "扩展二"]);
    // 与既有查询等值的扩展被丢弃（受控去重，不算失败）。
    expect(parseQueryExpansion('{"expansions":["原问题","扩展一"]}', ["原问题"])).toEqual(["扩展一"]);
    expect(() => parseQueryExpansion('{"expansions":["a","b","c","d"]}', [])).toThrowError(
      expect.objectContaining({ code: "KNOWLEDGE_MODEL_OUTPUT_INVALID" }),
    );
    expect(() => parseQueryExpansion("not json", [])).toThrow();
    expect(() => parseQueryExpansion('{"expansions":["  "]}', [])).toThrow();
    // 宽容输入 + 严格消费（2026-08-30 拆解优化）：未知字段忽略不整体拒绝，
    // 白名单只消费 expansions——无害格式偏差不再浪费一次 8s 纠错。
    expect(parseQueryExpansion('{"expansions":["x"],"extra":1}', [])).toEqual(["x"]);
    // 必需字段缺失仍拒绝。
    expect(() => parseQueryExpansion('{"extra":1}', [])).toThrow();
    // Markdown 围栏包裹被程序剥离，不算失败（§14 格式错误不走 LLM 纠错）。
    expect(parseQueryExpansion('```json\n{"expansions":["x"]}\n```', [])).toEqual(["x"]);
  });

  it("扩展成功：扩展查询与子查询同样并行检索、进 RRF；块内列出采纳的扩展", async () => {
    const expandModel: QueryExpansionModel = async () =>
      JSON.stringify({ expansions: ["同义改写一", "同义改写二", "实体归一化"] });
    const queries: string[] = [];
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        decomposeModel: DECOMPOSE_MODEL,
        expandModel,
        retrieve: async ({ query }) => {
          queries.push(query);
          return fakeRetrieval([fakeChunk({ id: `c-${query}`, text: `证据-${query}` })]);
        },
      },
    });
    // 直检 1 + 子查询 2 + 扩展 3 = 6 次并行检索（共享总预算）。
    expect(queries).toEqual(["问题", "子查询甲", "子查询乙", "同义改写一", "同义改写二", "实体归一化"]);
    expect(block).toContain("Query expansions (controlled):");
    expect(block).toContain("- 同义改写一");
    expect(stats.expandedQueries).toEqual(["同义改写一", "同义改写二", "实体归一化"]);
    expect(stats.expandedQueryHits).toEqual([1, 1, 1]);
    expect(stats.expansionDegradeReason).toBeUndefined();
    // 6 条名次序列各贡献 1 个唯一 chunk。
    expect(stats.uniqueChunkCount).toBe(6);
  });

  it("扩展输出与既有查询重复：去重后不重复检索", async () => {
    const expandModel: QueryExpansionModel = async () =>
      JSON.stringify({ expansions: ["问题", "子查询甲", "子查询甲"] });
    const queries: string[] = [];
    const { stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        decomposeModel: DECOMPOSE_MODEL,
        expandModel,
        retrieve: async ({ query }) => {
          queries.push(query);
          return fakeRetrieval([fakeChunk({ id: `c-${query}` })]);
        },
      },
    });
    expect(queries).toEqual(["问题", "子查询甲", "子查询乙"]);
    expect(stats.expandedQueries).toEqual([]);
  });

  it("输出 >3 条：纠错重试一次，修正后采纳；仍无效则降级留痕不扩展", async () => {
    const calls: string[] = [];
    const expandModel: QueryExpansionModel = async ({ correction }) => {
      calls.push(correction ? "retry" : "first");
      return correction
        ? JSON.stringify({ expansions: ["修正扩展"] })
        : JSON.stringify({ expansions: ["a", "b", "c", "d"] });
    };
    const queries: string[] = [];
    const { stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        decomposeModel: DECOMPOSE_MODEL,
        expandModel,
        retrieve: async ({ query }) => {
          queries.push(query);
          return fakeRetrieval([]);
        },
      },
    });
    expect(calls).toEqual(["first", "retry"]);
    expect(stats.expandedQueries).toEqual(["修正扩展"]);

    const failing: QueryExpansionModel = async () => "still not json";
    const second = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        decomposeModel: DECOMPOSE_MODEL,
        expandModel: failing,
        retrieve: async ({ query }) => {
          queries.push(query);
          return fakeRetrieval([]);
        },
      },
    });
    expect(second.stats.expansionDegradeReason).toBe("model output invalid after one correction retry");
    expect(second.stats.expandedQueries).toEqual([]);
    expect(second.block).toContain("[query expansion unavailable: model output invalid after one correction retry]");
  });

  it("扩展模型未接线 / 调用失败：不扩展、无额外检索、显式留痕（禁静默）", async () => {
    const queries: string[] = [];
    const base = {
      decomposeModel: DECOMPOSE_MODEL,
      retrieve: async ({ query }: { query: string }) => {
        queries.push(query);
        return fakeRetrieval([]);
      },
    };
    const noModel = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: { ...base, expandModel: null },
    });
    expect(noModel.stats.expansionDegradeReason).toBe("knowledge model slot not configured");
    expect(noModel.block).toContain("[query expansion unavailable: knowledge model slot not configured]");
    // 未扩展：只有直检 + 2 子查询。
    expect(queries).toEqual(["问题", "子查询甲", "子查询乙"]);

    queries.length = 0;
    const failing = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        ...base,
        expandModel: async () => {
          throw new Error("rate limited");
        },
      },
    });
    expect(failing.stats.expansionDegradeReason).toBe("model call failed");
    expect(queries).toEqual(["问题", "子查询甲", "子查询乙"]);
  });

  it("拆解降级（无模型）时不尝试扩展：无扩展留痕（拆解留痕已覆盖原因）", async () => {
    const { stats, block } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        decomposeModel: null,
        retrieve: async () => fakeRetrieval([fakeChunk()]),
      },
    });
    expect(stats.expansionDegradeReason).toBeUndefined();
    expect(block).not.toContain("query expansion unavailable");
  });

  it("expandQueries 纯函数：连续无效降级细节与 decomposeQuestion 同风格", async () => {
    const result = await expandQueries({
      question: "问题",
      existingQueries: ["子查询甲"],
      callModel: null,
    });
    expect(result).toMatchObject({
      expansions: [],
      attempted: false,
      degraded: true,
      degradeReason: "knowledge model slot not configured",
    });
    // 拆解层对照：无模型同样显式留痕（§三十四 安全网不因扩展缺失而弱化）。
    const decomposition = await decomposeQuestion({ question: "问题", callModel: null });
    expect(decomposition.degraded).toBe(true);
  });
});

// ── §二十六 Candidate budgets + §三十六 邻接扩展 ─────────────────────────

describe("HIGH_RECALL：预算链与邻接扩展（§二十六/§三十六/§九十四）", () => {
  it("fuseSubQueryResults 融合池按 fusionBudget 截断", () => {
    const candidates = Array.from({ length: 100 }, (_, index) => fakeChunk({ id: `c${index}` }));
    const fused = fuseSubQueryResults([fakeRetrieval(candidates)]);
    expect(fused).toHaveLength(KNOWLEDGE_FUSION_BUDGET);
    expect(fused[0].id).toBe("c0"); // 截断保序：RRF 名次前列保留
  });

  it("candidate budget 链逐级截断并留痕计数：150 候选 → 融合池/锚点随预算伸缩", async () => {
    const candidates = Array.from({ length: 150 }, (_, index) => fakeChunk({ id: `c${index}`, ordinal: index }));
    // 显式预算（8000）覆盖每条证据新增的外部内容边界开销：融合池上限
    // = min(480, 8000×0.7/5) = 480 → 150 候选全部入池；锚点上限
    // = min(240, 8000×0.5/5) = 240 → 全部成为锚点
    // （2026-08-30 阀 A/阀 B 均随预算倒推后的语义；真实块 ~1300 token 时池子
    // 仍在 60 水位地板，见 resolveFusionPoolBudget 单测）。
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      budgetTokens: 8000,
      deps: {
        decomposeModel: null,
        retrieve: async () => fakeRetrieval(candidates),
      },
    });
    expect(stats.candidateChunkCount).toBe(150);
    expect(stats.uniqueChunkCount).toBe(150);
    expect(stats.fusedChunks).toBe(150);
    expect(stats.injectedChunks).toBe(150);
    // 融合候选全部成为锚点：无 "beyond the evidence budget" 截断留痕。
    expect(block).not.toContain("fused candidates beyond the evidence budget");
    expect((block.match(/\[K\d+\] notebook/g) || []).length).toBe(150);
  });

  it("邻接扩展 ±1：contextOnly 标记、计入 neighborExpansionCount、不计检索命中/footprint 分子", async () => {
    const anchorA = fakeChunk({ id: "a1", ordinal: 1, text: "锚点一" });
    const anchorB = fakeChunk({ id: "a2", ordinal: 2, text: "锚点二" });
    const neighborBefore = fakeChunk({ id: "n0", ordinal: 0, text: "前邻接块" });
    const neighborAfter = fakeChunk({ id: "n3", ordinal: 3, text: "后邻接块" });
    const readCalls: Array<{ anchorOrdinal: number; ordinals: number[] }> = [];
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        decomposeModel: null,
        retrieve: async () => fakeRetrieval([anchorA, anchorB]),
        readNeighborChunks: ({ anchor, ordinals }) => {
          readCalls.push({ anchorOrdinal: anchor.ordinal, ordinals: [...ordinals] });
          const pool = [neighborBefore, anchorA, anchorB, neighborAfter];
          return pool.filter(chunk => ordinals.includes(chunk.ordinal));
        },
      },
    });
    // 每个锚点请求 ±1 窗口（默认 KNOWLEDGE_NEIGHBOR_EXPANSION_WINDOW = 1）。
    expect(KNOWLEDGE_NEIGHBOR_EXPANSION_WINDOW).toBe(1);
    expect(readCalls).toEqual([
      { anchorOrdinal: 1, ordinals: [0, 2] },
      { anchorOrdinal: 2, ordinals: [1, 3] },
    ]);
    // 检索命中只有 2 个锚点；邻接块不进 fusedChunks / uniqueChunkCount。
    expect(stats.fusedChunks).toBe(2);
    expect(stats.uniqueChunkCount).toBe(2);
    expect(stats.neighborExpansionCount).toBe(2);
    // 注入序：锚点一、前邻接、锚点二、后邻接；邻接块带 contextOnly 与锚点标注。
    expect(stats.results).toEqual([
      { ordinal: 1, sourceName: "源", chunkOrdinal: 2, firstLine: "锚点一" },
      { ordinal: 2, sourceName: "源", chunkOrdinal: 1, firstLine: "前邻接块", contextOnly: true },
      { ordinal: 3, sourceName: "源", chunkOrdinal: 3, firstLine: "锚点二" },
      { ordinal: 4, sourceName: "源", chunkOrdinal: 4, firstLine: "后邻接块", contextOnly: true },
    ]);
    expect(block).toContain("context-only neighbor of [K1]");
    expect(block).toContain("context-only neighbor of [K3]");
  });

  it("多渠道/邻接不重复注入同一 chunk：锚点互为邻接时只按锚点注入一次", async () => {
    const shared = fakeChunk({ id: "shared", ordinal: 5, text: "共享块" });
    const adjacent = fakeChunk({ id: "adjacent", ordinal: 6, text: "邻接即锚点" });
    const far = fakeChunk({ id: "far", ordinal: 40, text: "远处块" });
    const { stats, block } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        decomposeModel: async () => validDecomposeOutput(["子查询一", "子查询二"]),
        retrieve: async ({ query }) => fakeRetrieval(
          query === "子查询一" ? [shared, adjacent] : [shared, adjacent, far],
        ),
        readNeighborChunks: ({ anchor, ordinals }) => [shared, adjacent, far]
          .filter(chunk => ordinals.includes(chunk.ordinal) && chunk.parseArtifactId === anchor.parseArtifactId),
      },
    });
    // 两条子查询同名次命中同一 chunk：RRF 去重后唯一 3 个；shared/adjacent 互为
    // 邻接（ordinal 5/6）——邻接请求命中的块已是注入锚点 → 不重复注入；
    // far 的邻接 ordinal（39/41）在语料中缺席 → 无邻接块注入。
    expect(stats.uniqueChunkCount).toBe(3);
    expect(stats.neighborExpansionCount).toBe(0);
    const headerCount = (block.match(/\[K\d+\] notebook/g) ?? []).length;
    expect(stats.injectedChunks).toBe(headerCount);
    expect(stats.injectedChunks).toBe(3);
  });

  it("readNeighborChunks 未接线：不扩展、stats.neighborExpansionCount = 0（调用方未启用）", async () => {
    const { stats, block } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        decomposeModel: null,
        retrieve: async () => fakeRetrieval([fakeChunk({ id: "a1", ordinal: 1 })]),
      },
    });
    expect(stats.neighborExpansionCount).toBe(0);
    expect(block).not.toContain("context-only");
  });

  it("邻接块放不下预算只跳过自身，不截断锚点链", async () => {
    const anchors = Array.from({ length: 6 }, (_, index) => fakeChunk({
      id: `a${index}`,
      ordinal: index * 10,
      text: `锚点${index}-${"证".repeat(30)}`,
    }));
    const neighbors = anchors.map((anchor, index) => fakeChunk({
      id: `n${index}`,
      ordinal: index * 10 + 1,
      text: `邻接${index}-${"上下".repeat(1200)}`, // 超大邻接块：任何预算都放不下
    }));
    const { stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      budgetTokens: 900,
      deps: {
        decomposeModel: null,
        retrieve: async () => fakeRetrieval(anchors),
        readNeighborChunks: ({ anchor }) => {
          const index = anchors.findIndex(candidate => candidate.id === anchor.id);
          return [neighbors[index]].filter(chunk => chunk.parseArtifactId === anchor.parseArtifactId);
        },
      },
    });
    // 邻接块全部放不下 → 0 个注入；锚点照常全部注入（不被上下文块挤掉）。
    expect(stats.neighborExpansionCount).toBe(0);
    expect(stats.injectedChunks).toBe(anchors.length);
    expect(stats.truncated).toBe(false);
    expect(stats.results?.every(entry => entry.contextOnly !== true)).toBe(true);
  });
});

// ── §三十七~§三十九 BROAD / §四十一 自动升级与 exhaustive 降格 ────────────

describe("BROAD：Source Coverage Floor 与 Section Coverage（§九十五）", () => {
  const SOURCE_A = fakeSource({ sourceId: "src-a", sourceName: "A.pdf", parseArtifactId: "parse-a", chunkCount: 50 });
  const SOURCE_B = fakeSource({ sourceId: "src-b", sourceName: "B.pdf", parseArtifactId: "parse-b", chunkCount: 4 });
  const SOURCE_C = fakeSource({ sourceId: "src-c", sourceName: "C.pdf", parseArtifactId: "parse-c", chunkCount: 4 });
  const SOURCE_D = fakeSource({ sourceId: "src-d", sourceName: "D.pdf", parseArtifactId: "parse-d", chunkCount: 4 });
  const ALL_SOURCES = [SOURCE_A, SOURCE_B, SOURCE_C, SOURCE_D];
  const A_CHUNKS = Array.from({ length: 5 }, (_, index) => fakeChunk({
    id: `a${index}`,
    parseArtifactId: "parse-a",
    chunkIndexVariantId: "civ-a",
    sourceId: "src-a",
    sourceName: "A.pdf",
    ordinal: index,
    text: `A 源证据 ${index}`,
  }));

  /** 主查询全量检索：A 命中 + 全源清单；约束检索（sourceIds 给定）单独记录。 */
  function createFourSourceFacade() {
    const constrainedCalls: Array<{ query: string; sourceIds?: string[]; sectionsBySourceId?: Map<string, string[]> }> = [];
    const retrieve = async ({ query, sourceIds, sectionsBySourceId }: {
      query: string;
      sourceIds?: string[];
      sectionsBySourceId?: ReadonlyMap<string, string[]>;
    }): Promise<RetrieveForNotebooksResult> => {
      if (sourceIds) {
        constrainedCalls.push({ query, sourceIds: [...sourceIds], sectionsBySourceId: sectionsBySourceId ? new Map(sectionsBySourceId) : undefined });
        return fakeRetrieval([], ALL_SOURCES.filter(source => sourceIds.includes(source.sourceId)));
      }
      return fakeRetrieval([...A_CHUNKS], ALL_SOURCES);
    };
    return { retrieve, constrainedCalls };
  }

  it("4 源 A 极相关、B/C/D 零命中：broad 主动探测 B/C/D（constrained retrieval 被调用）", async () => {
    const { retrieve, constrainedCalls } = createFourSourceFacade();
    const { stats } = await buildKnowledgeContextInjection({
      question: "这些文件分别如何看待 X？",
      mode: "detailed",
      deps: { decomposeModel: null, retrieve },
      coveragePlan: planOf({ coverageMode: "broad", intent: "cross_source_synthesis", scopeLevel: "multi_source" }),
    });
    const probedSources = new Set(constrainedCalls.flatMap(call => call.sourceIds ?? []));
    expect(probedSources).toEqual(new Set(["src-b", "src-c", "src-d"]));
    expect(constrainedCalls.every(call => call.sectionsBySourceId == null)).toBe(true);
    expect(stats.secondaryRetrievalCount).toBe(constrainedCalls.length);
    expect(stats.executedCoverageMode).toBe("broad");
  });

  it("B/C/D 确无相关证据：块内显式 no relevant evidence、无硬塞低质 chunk", async () => {
    const { retrieve } = createFourSourceFacade();
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "这些文件分别如何看待 X？",
      mode: "detailed",
      deps: { decomposeModel: null, retrieve },
      coveragePlan: planOf({ coverageMode: "broad", intent: "cross_source_synthesis", scopeLevel: "multi_source" }),
    });
    expect(block).toContain('[no relevant evidence found in source "B.pdf" (sourceId: src-b)]');
    expect(block).toContain('[no relevant evidence found in source "C.pdf" (sourceId: src-c)]');
    expect(block).toContain('[no relevant evidence found in source "D.pdf" (sourceId: src-d)]');
    // 候选仍只来自 A：绝不为配额硬塞 B/C/D 的无关 chunk。
    expect(stats.results?.every(entry => entry.sourceName === "A.pdf")).toBe(true);
    expect(stats.retrievedSourceCount).toBe(1);
    expect(stats.selectedSourceCount).toBe(4);
    expect(stats.sourceCoverageFootprint).toBe(0.25);
  });

  it("section coverage：跨章节命中集中一节 → 对未命中 section 做 constrained 二次检索", async () => {
    const SECTIONS = ["第一章 起点", "第二章 转折", "第三章 高潮", "第四章 结局"];
    const sourceWithSections = fakeSource({
      sourceId: "src-s",
      sourceName: "长篇.md",
      parseArtifactId: "parse-s",
      chunkCount: 8,
      sections: SECTIONS,
    });
    const sectionHits = [fakeChunk({
      id: "s1",
      parseArtifactId: "parse-s",
      sourceId: "src-s",
      sourceName: "长篇.md",
      ordinal: 0,
      headingPath: ["第一章 起点"],
    })];
    const constrainedCalls: Array<{ sourceIds?: string[]; sectionsBySourceId?: Map<string, string[]> }> = [];
    const { stats, block } = await buildKnowledgeContextInjection({
      question: "这本书整体如何演进？",
      mode: "detailed",
      deps: {
        decomposeModel: null,
        retrieve: async ({ sourceIds, sectionsBySourceId }) => {
          if (sourceIds) {
            constrainedCalls.push({ sourceIds: [...sourceIds], sectionsBySourceId: sectionsBySourceId ? new Map(sectionsBySourceId) : undefined });
            // 未命中章节确无相关证据：返回空（不硬塞）。
            return fakeRetrieval([], [sourceWithSections]);
          }
          return fakeRetrieval([...sectionHits], [sourceWithSections]);
        },
      },
      coveragePlan: planOf({ coverageMode: "broad", intent: "whole_scope_analysis", scopeLevel: "source" }),
    });
    // 命中 1/4 < 0.5 阈值：对未命中三章做 section-constrained 探测。
    expect(constrainedCalls).toHaveLength(1);
    expect(constrainedCalls[0].sourceIds).toEqual(["src-s"]);
    expect(constrainedCalls[0].sectionsBySourceId?.get("src-s")).toEqual(["第二章 转折", "第三章 高潮", "第四章 结局"]);
    expect(stats.secondaryRetrievalCount).toBe(1);
    expect(stats.availableSectionCount).toBe(4);
    expect(stats.retrievedSectionCount).toBe(1);
    expect(stats.sectionCoverageFootprint).toBe(0.25);
    expect(block).toContain('[no relevant evidence found in sections "第二章 转折", "第三章 高潮", "第四章 结局" of source "长篇.md" (sourceId: src-s)]');
  });

  it("scopeLevel=local 的点问题不触发 section 探测（启发式要求整体性）", async () => {
    const SECTIONS = ["第一章", "第二章", "第三章", "第四章"];
    const sourceWithSections = fakeSource({ sourceId: "src-s", sections: SECTIONS, chunkCount: 8 });
    const constrainedCalls: Array<{ sourceIds?: string[] }> = [];
    const { stats } = await buildKnowledgeContextInjection({
      question: "第二章里的那个日期是什么？",
      mode: "detailed",
      deps: {
        decomposeModel: null,
        retrieve: async ({ sourceIds }) => {
          if (sourceIds) {
            constrainedCalls.push({ sourceIds: [...sourceIds] });
            return fakeRetrieval([], [sourceWithSections]);
          }
          return fakeRetrieval([fakeChunk({ sourceId: "src-s", headingPath: ["第二章"] })], [sourceWithSections]);
        },
      },
      coveragePlan: planOf({ coverageMode: "broad", scopeLevel: "local" }),
    });
    expect(constrainedCalls).toHaveLength(0);
    expect(stats.secondaryRetrievalCount).toBe(0);
    expect(stats.retrievedSectionCount).toBe(1);
  });

  it("自动升级（§四十一 执行侧）：footprint 不足 → 补 broad 轮 + stats.upgradedTo", async () => {
    const { retrieve, constrainedCalls } = createFourSourceFacade();
    const { stats, block } = await buildKnowledgeContextInjection({
      question: "X 的相关记录都在哪？",
      mode: "detailed",
      deps: { decomposeModel: null, retrieve },
      coveragePlan: planOf({ coverageMode: "high_recall", scopeLevel: "multi_source" }),
    });
    expect(stats.upgradedTo).toBe("broad");
    expect(stats.executedCoverageMode).toBe("broad");
    expect(block).toContain(`[coverage auto-upgrade: high_recall → broad (source coverage footprint 0.25 below ${KNOWLEDGE_AUTO_UPGRADE_SOURCE_FOOTPRINT_MIN})]`);
    // 复用已检索结果只补缺失探测：A 不重探，只探 B/C/D。
    expect(new Set(constrainedCalls.flatMap(call => call.sourceIds ?? []))).toEqual(new Set(["src-b", "src-c", "src-d"]));
  });

  it("footprint 达标或单源 scope 不自动升级", async () => {
    // 全源命中：footprint = 1，不升级。
    const allHit = async (): Promise<RetrieveForNotebooksResult> => fakeRetrieval([
      ...A_CHUNKS,
      fakeChunk({ id: "b1", sourceId: "src-b", sourceName: "B.pdf", parseArtifactId: "parse-b" }),
      fakeChunk({ id: "c1", sourceId: "src-c", sourceName: "C.pdf", parseArtifactId: "parse-c" }),
      fakeChunk({ id: "d1", sourceId: "src-d", sourceName: "D.pdf", parseArtifactId: "parse-d" }),
    ], ALL_SOURCES);
    const full = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: { decomposeModel: null, retrieve: allHit },
      coveragePlan: planOf({ coverageMode: "high_recall", scopeLevel: "multi_source" }),
    });
    expect(full.stats.upgradedTo).toBeUndefined();
    expect(full.stats.sourceCoverageFootprint).toBe(1);

    // 单源 scope：低于阈值也不升级（§四十一 要求多源）。
    const singleSource = fakeSource({ sourceId: "src-only", chunkCount: 9 });
    const lone = fakeChunk({ sourceId: "src-only" });
    const single = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        decomposeModel: null,
        retrieve: async () => fakeRetrieval([lone], [singleSource]),
      },
      coveragePlan: planOf({ coverageMode: "high_recall", scopeLevel: "source" }),
    });
    expect(single.stats.upgradedTo).toBeUndefined();
    expect(single.stats.secondaryRetrievalCount).toBe(0);
  });

  it("存量旧值 exhaustive 计划：直接按 broad 执行（结构探测照常），无降格留痕负担", async () => {
    const { retrieve, constrainedCalls } = createFourSourceFacade();
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "全文提到的风险都列出来",
      mode: "detailed",
      deps: { decomposeModel: null, retrieve },
      coveragePlan: planOf({
        coverageMode: "exhaustive",
        intent: "whole_scope_analysis",
        scopeLevel: "multi_source",
      }),
    });
    // 块头如实透出存量计划档位；执行侧按 broad（exhaustive 档已移除）。
    expect(stats.coverageMode).toBe("exhaustive");
    expect(stats.executedCoverageMode).toBe("broad");
    expect(stats).not.toHaveProperty("coverageDegradeReason");
    expect(block).toContain("[coverage: exhaustive · multi_source]");
    // broad 档结构探测照常执行。
    expect(constrainedCalls.length).toBeGreaterThan(0);
  });

  it("无 planner（旧路径）：不执行 broad 探测、stats 无执行档位字段", async () => {
    const { retrieve, constrainedCalls } = createFourSourceFacade();
    const { stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: { decomposeModel: null, retrieve },
    });
    expect(constrainedCalls).toHaveLength(0);
    expect(stats.secondaryRetrievalCount).toBe(0);
    expect(stats).not.toHaveProperty("executedCoverageMode");
    expect(stats).not.toHaveProperty("upgradedTo");
    // §四十：普通检索也记录 footprint 计数（chunkRecallFootprint 是触达率，非 actual recall）。
    expect(stats.selectedSourceCount).toBe(4);
    expect(stats.retrievedSourceCount).toBe(1);
    expect(stats.sourceCoverageFootprint).toBe(0.25);
  });

  it("索引不可用的源不记 no relevant evidence（已有降级留痕，不冒充无证据）", async () => {
    const healthy = fakeSource({ sourceId: "src-ok", sourceName: "OK.pdf", chunkCount: 5 });
    const broken = fakeSource({ sourceId: "src-broken", sourceName: "Broken.pdf", parseArtifactId: "parse-broken", chunkCount: 0 });
    const hit = fakeChunk({ sourceId: "src-ok", sourceName: "OK.pdf" });
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "detailed",
      deps: {
        decomposeModel: null,
        retrieve: async ({ sourceIds }) => {
          if (sourceIds) {
            return {
              candidates: [],
              sources: [broken],
              retrievalMode: "fts" as const,
              retrievalModeRequested: "fts" as const,
              degraded: [{
                parseArtifactId: "parse-broken",
                chunkProfileHash: "",
                reason: "KNOWLEDGE_INDEX_MISSING" as const,
                sourceId: "src-broken",
                sourceName: "Broken.pdf",
              }],
            };
          }
          return {
            candidates: [hit],
            sources: [healthy, broken],
            retrievalMode: "fts" as const,
            retrievalModeRequested: "fts" as const,
            degraded: [],
          };
        },
      },
      coveragePlan: planOf({ coverageMode: "broad", scopeLevel: "multi_source" }),
    });
    expect(block).not.toContain('no relevant evidence found in source "Broken.pdf"');
    expect(block).toContain("knowledge retrieval degraded: KNOWLEDGE_INDEX_MISSING");
    expect(stats.degradedScopes?.length).toBeGreaterThan(0);
  });
});

// ── 真实 KnowledgeManager 集成：section 约束检索与邻接块回读 ───────────────

describe("retrieveForNotebooks 约束检索与 readAdjacentChunks（真实索引库）", () => {
  async function setupMarkdownManager() {
    const manager = new KnowledgeManager({ lingxiHome: tempHome() });
    managers.push(manager);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "长文档" });
    const chapters = [
      "# 第一章 信标\n\n信标阵列的初始坐标记录在第一章。信标阵列的初始坐标记录在第一章。",
      "# 第二章 航路\n\n航路规划依赖季风窗口与洋流节奏。航路规划依赖季风窗口与洋流节奏。",
      "# 第三章 补给\n\n补给点的分布决定了远航的极限半径。补给点的分布决定了远航的极限半径。",
      "# 第四章 归档\n\n归档记录按年度封存并移交档案馆。归档记录按年度封存并移交档案馆。",
    ];
    // markdown 结构元数据（headingPath）来自文件导入路径：外部 .md 文件
    // （lingxiHome 内部路径会被导入安全拦截，与既有 injector 测试同一模式）。
    const outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-coverage-exec-outside-"));
    tempDirs.push(outsideDir);
    const filePath = path.join(outsideDir, "航行日志.md");
    fs.writeFileSync(filePath, chapters.join("\n\n"));
    const imported = await manager.importFile({ studioId, notebookId: notebook.id, filePath });
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    manager.queryService.indexArtifactForIngestion(studioId, artifact.id, {
      targetChars: manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id }),
    });
    return { manager, studioId, notebook, artifact };
  }

  it("sourceIds 约束：只检索指定源、清单只含该源", async () => {
    const { manager, studioId, notebook } = await setupMarkdownManager();
    const other = manager.createNotebook({ studioId, name: "其他" });
    const imported = await manager.importPastedText({
      studioId,
      notebookId: other.id,
      text: "信标阵列也出现在别的笔记本。",
      displayName: "别的.txt",
    });
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    manager.queryService.indexArtifactForIngestion(studioId, artifact.id, {
      targetChars: manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: other.id }),
    });
    const otherSources = manager.store.listNotebookSources({ studioId, notebookId: other.id });
    expect(otherSources.length).toBe(1);
    // 约束到主源 → 只命中主源 chunk，另一笔记本的同词内容不进候选。
    const allSources = manager.store.listNotebookSources({ studioId, notebookId: notebook.id });
    const constrained = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id, other.id],
      question: "信标",
      sourceIds: [allSources[0].source.id],
    });
    expect(constrained.sources.map(source => source.sourceName)).toEqual(["航行日志.md"]);
    expect(constrained.candidates.length).toBeGreaterThan(0);
    expect(constrained.candidates.every(chunk => chunk.sourceName === "航行日志.md")).toBe(true);
  });

  it("sectionsBySourceId 约束：FTS 只命中选中章节的 chunk", async () => {
    const { manager, studioId, notebook } = await setupMarkdownManager();
    const sources = manager.store.listNotebookSources({ studioId, notebookId: notebook.id });
    const sourceId = sources[0].source.id;
    const unconstrained = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "信标",
    });
    // 无约束：命中第一章（信标所在章节）。
    expect(unconstrained.candidates.map(chunk => chunk.headingPath?.join(" > "))).toEqual(["第一章 信标"]);
    const sections = unconstrained.sources[0].sections ?? [];
    expect(sections).toEqual(["第一章 信标", "第二章 航路", "第三章 补给", "第四章 归档"]);

    // 约束到无命中的章节：返回空（SQL 层 ordinal 过滤，非 post-filter 碰巧为空）。
    const emptyConstrained = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "信标",
      sourceIds: [sourceId],
      sectionsBySourceId: new Map([[sourceId, ["第二章 航路", "第三章 补给"]]]),
    });
    expect(emptyConstrained.candidates).toEqual([]);

    // 约束键包含命中章节：正常返回。
    const hitConstrained = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "信标",
      sourceIds: [sourceId],
      sectionsBySourceId: new Map([[sourceId, ["第一章 信标", "第四章 归档"]]]),
    });
    expect(hitConstrained.candidates.length).toBeGreaterThan(0);
    expect(hitConstrained.candidates.every(chunk => chunk.headingPath?.[0] === "第一章 信标")).toBe(true);

    // 不存在的 section 键：区间为空 → 无结果（不猜测、不回退全源）。
    const missing = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "信标",
      sourceIds: [sourceId],
      sectionsBySourceId: new Map([[sourceId, ["不存在的章节"]]]),
    });
    expect(missing.candidates).toEqual([]);
  });

  it("readAdjacentChunks：同变体 ±1 定点回读并附定位注解", async () => {
    const { manager, studioId, notebook } = await setupMarkdownManager();
    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "信标",
    });
    const anchor = result.candidates[0];
    const neighbors = manager.queryService.readAdjacentChunks({
      studioId,
      anchor: {
        notebookId: anchor.notebookId,
        notebookName: anchor.notebookName,
        sourceId: anchor.sourceId,
        sourceName: anchor.sourceName,
        parseArtifactId: anchor.parseArtifactId,
        chunkIndexVariantId: anchor.chunkIndexVariantId,
      },
      ordinals: [anchor.ordinal + 1],
    });
    expect(neighbors.length).toBeGreaterThan(0);
    for (const neighbor of neighbors) {
      expect(neighbor.parseArtifactId).toBe(anchor.parseArtifactId);
      expect(neighbor.chunkIndexVariantId).toBe(anchor.chunkIndexVariantId);
      expect(Math.abs(neighbor.ordinal - anchor.ordinal)).toBe(1);
      expect(neighbor.notebookId).toBe(anchor.notebookId);
      expect(neighbor.sourceId).toBe(anchor.sourceId);
      expect(neighbor.score).toBe(0);
    }
    // 越界 ordinal（源只有 4 chunk）：缺席不报错。
    expect(manager.queryService.readAdjacentChunks({
      studioId,
      anchor: {
        notebookId: anchor.notebookId,
        notebookName: anchor.notebookName,
        sourceId: anchor.sourceId,
        sourceName: anchor.sourceName,
        parseArtifactId: anchor.parseArtifactId,
        chunkIndexVariantId: anchor.chunkIndexVariantId,
      },
      ordinals: [9999],
    })).toEqual([]);
  });

  it("端到端：broad 计划触发真实 section 探测（sectionsBySourceId 传到检索核心）", async () => {
    const { manager, studioId, notebook } = await setupMarkdownManager();
    const spy = vi.spyOn(manager.queryService, "retrieveForNotebooks");
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "信标",
      mode: "detailed",
      deps: {
        decomposeModel: null,
        retrieve: ({ query, sourceIds, sectionsBySourceId }) => manager.queryService.retrieveForNotebooks({
          studioId,
          notebookIds: [notebook.id],
          question: query,
          ...(sourceIds ? { sourceIds } : {}),
          ...(sectionsBySourceId ? { sectionsBySourceId } : {}),
        }),
        readNeighborChunks: ({ anchor, ordinals }) => manager.queryService.readAdjacentChunks({
          studioId,
          anchor: {
            notebookId: anchor.notebookId,
            notebookName: anchor.notebookName,
            sourceId: anchor.sourceId,
            sourceName: anchor.sourceName,
            parseArtifactId: anchor.parseArtifactId,
            chunkIndexVariantId: anchor.chunkIndexVariantId,
          },
          ordinals,
        }),
      },
      coveragePlan: planOf({ coverageMode: "broad", intent: "whole_scope_analysis", scopeLevel: "source" }),
    });
    // 主查询命中只有"信标"章节（第一章）→ 1/4 < 0.5 → 真实 section 探测发生。
    const sectionCalls = spy.mock.calls.filter(call => call[0].sectionsBySourceId);
    expect(sectionCalls.length).toBeGreaterThan(0);
    const probedSections = sectionCalls[0][0].sectionsBySourceId!.values().next().value;
    expect(probedSections).toContain("第二章 航路");
    expect(probedSections).toContain("第四章 归档");
    expect(stats.secondaryRetrievalCount).toBe(sectionCalls.length);
    expect(stats.availableSectionCount).toBe(4);
    expect(stats.retrievedSectionCount).toBe(1);
    expect(stats.sectionCoverageFootprint).toBe(0.25);
    // 未命中章节确无"整本日志的整体脉络"相关词 → 如实记录，不硬塞。
    expect(block).toContain('no relevant evidence found in sections');
    // 邻接扩展：真实库回读的上下文块注入（contextOnly）。
    expect(stats.neighborExpansionCount).toBeGreaterThan(0);
    expect(stats.results?.some(entry => entry.contextOnly === true)).toBe(true);
    spy.mockRestore();
  });
});
