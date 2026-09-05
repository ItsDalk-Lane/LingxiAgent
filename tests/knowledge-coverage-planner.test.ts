/**
 * KnowledgeCoveragePlanner（任务书 §二十七–§三十二/§九十六，Phase 7；
 * 2026-08-31 两档化改写）：
 * - 第一层确定性规则：全库/完整性关键词与 global-negative 句式 → broad（历史
 *   exhaustive 定档改道）；多源指代 → broad+multi_source；单点事实 → high_recall；
 * - 第二层语义判断：whole_scope_analysis/global_negative 意图与模型旧值 exhaustive
 *   输出一律归并 broad；输出非法/调用失败降级 high_recall 并留痕；
 * - 三维度正交：plan 不携带 answerMode/retrievalMode；
 * - 持久化 round-trip（schema v13 knowledge_coverage_plans；requires_completeness
 *   遗留列新行恒 false）；
 * - injector 集成回归：块头 coverage 标注 + stats 透出，检索行为与无 planner 一致。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT,
  RULE_EXHAUSTIVE_KEYWORD,
  RULE_FACT_LOOKUP,
  RULE_GLOBAL_NEGATIVE,
  RULE_MULTI_SOURCE,
  matchCoverageRules,
  parseCoverageClassification,
  planKnowledgeCoverage,
  type CoverageClassifyModel,
  type KnowledgeCoveragePlan,
} from "./fixtures/knowledge-legacy/legacy-coverage-planner.ts";
import { buildKnowledgeContextInjection } from "./fixtures/knowledge-legacy/legacy-knowledge-context-injector.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { KNOWLEDGE_SCHEMA_VERSION, KnowledgeStore } from "../lib/knowledge/knowledge-store.ts";
import type { RetrieveForNotebooksResult } from "../lib/knowledge/knowledge-query-service.ts";

const require = createRequire(import.meta.url);
const Database = require("better-sqlite3");
const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];
const stores: KnowledgeStore[] = [];

afterEach(async () => {
  for (const manager of managers.splice(0)) await manager.close();
  for (const store of stores.splice(0)) store.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function tempHome(prefix: string) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

function classificationJson(overrides: Record<string, unknown> = {}) {
  return JSON.stringify({
    intent: "whole_scope_analysis",
    coverageMode: "broad",
    scopeLevel: "notebook",
    confidence: 0.9,
    ...overrides,
  });
}

/** plan 不得携带 answerMode/retrievalMode/requiresCompleteness（三维度正交 + 两档化）。 */
function expectOrthogonal(plan: KnowledgeCoveragePlan) {
  for (const key of Object.keys(plan)) {
    expect(key).not.toBe("answerMode");
    expect(key).not.toBe("retrievalMode");
    expect(key).not.toBe("requiresCompleteness");
  }
}

describe("第一层确定性规则（§三十一，两档化）", () => {
  it.each([
    "请把这本书全文梳理一遍，不要遗漏任何论点",
    "列出所有提到交付日期的段落",
    "从头到尾全面分析这份报告",
    "第三章之后的全部内容都检查一遍",
  ])("全库/完整性关键词 → broad（无 LLM 也命中）：%s", async (question) => {
    const plan = await planKnowledgeCoverage({ question });
    expect(plan.coverageMode).toBe("broad");
    expect(plan.matchedRuleIds).toContain(RULE_EXHAUSTIVE_KEYWORD);
    expect(plan.intent).toBe("whole_scope_analysis");
    expect(plan.classifierUsed).toBe("rules");
    expect(plan.confidence).toBeGreaterThanOrEqual(0.8);
    expectOrthogonal(plan);
  });

  it.each([
    "全文有没有任何地方提到风险准备金？",
    "是否存在任何反例推翻这个结论？",
    "所有提到的交付节点是否都有出处？",
  ])("global-negative 句式 → broad + global_negative 意图：%s", async (question) => {
    const plan = await planKnowledgeCoverage({ question });
    expect(plan.coverageMode).toBe("broad");
    expect(plan.matchedRuleIds).toContain(RULE_GLOBAL_NEGATIVE);
    expect(plan.intent).toBe("global_negative");
    expect(plan.classifierUsed).toBe("rules");
    expectOrthogonal(plan);
  });

  it("关键词命中后 classifyModel 仍被复核（definitive 短路已移除）", async () => {
    const classifyModel = vi.fn(async () => classificationJson());
    const plan = await planKnowledgeCoverage({
      question: "全书所有出现的术语都列出来",
      classifyModel: classifyModel as unknown as CoverageClassifyModel,
    });
    expect(plan.coverageMode).toBe("broad");
    expect(plan.classifierUsed).toBe("rules+llm");
    expect(classifyModel).toHaveBeenCalledTimes(1);
  });

  it("多源指代 → broad + multi_source（无 LLM 即终稿）", async () => {
    const plan = await planKnowledgeCoverage({ question: "这几份文件分别如何看待利率风险？" });
    expect(plan.coverageMode).toBe("broad");
    expect(plan.scopeLevel).toBe("multi_source");
    expect(plan.matchedRuleIds).toContain(RULE_MULTI_SOURCE);
    expect(plan.intent).toBe("cross_source_synthesis");
    expect(plan.classifierUsed).toBe("rules");
    expectOrthogonal(plan);
  });

  it("普通事实问题 → high_recall + local", async () => {
    const plan = await planKnowledgeCoverage({ question: "项目是何时启动的？" });
    expect(plan.coverageMode).toBe("high_recall");
    expect(plan.scopeLevel).toBe("local");
    expect(plan.matchedRuleIds).toContain(RULE_FACT_LOOKUP);
    expect(plan.classifierUsed).toBe("rules");
    expectOrthogonal(plan);
  });

  it("无规则命中且无 classifyModel → 保守默认 high_recall 并留痕", async () => {
    const plan = await planKnowledgeCoverage({ question: "帮我看看这份材料讲什么" });
    expect(plan.coverageMode).toBe("high_recall");
    expect(plan.classifierUsed).toBe("rules");
    expect(plan.degradeReason).toBe("knowledge model slot not configured");
    expectOrthogonal(plan);
  });

  it("matchCoverageRules 的规则 id 稳定（可直接断言）", () => {
    expect(matchCoverageRules("全文怎么说的").matchedRuleIds).toEqual([RULE_EXHAUSTIVE_KEYWORD]);
    expect(matchCoverageRules("有没有任何遗漏").matchedRuleIds).toEqual([RULE_GLOBAL_NEGATIVE]);
    // 全库关键词与 global-negative 可叠加命中（两个 id 都在）。
    expect(matchCoverageRules("全文是否提到任何风险").matchedRuleIds)
      .toEqual([RULE_EXHAUSTIVE_KEYWORD, RULE_GLOBAL_NEGATIVE]);
  });
});

describe("scopeLevel 元数据推导（§三十）", () => {
  it.each([
    [{ notebookCount: 3, sourceCount: 7 }, "multi_notebook"],
    [{ notebookCount: 1, sourceCount: 4 }, "multi_source"],
    [{ notebookCount: 1, sourceCount: 1 }, "source"],
    [{ notebookCount: 1, sourceCount: null }, "notebook"],
    [{ notebookCount: null, sourceCount: 5 }, "whole_scope"],
  ])("元数据 %j → %s", async (info, expected) => {
    const plan = await planKnowledgeCoverage({
      question: "全文提到的风险条款都列出来",
      turnScopeInfo: info,
    });
    expect(plan.coverageMode).toBe("broad");
    expect(plan.scopeLevel).toBe(expected);
  });
});

describe("第二层语义判断（§三十二，两档化）", () => {
  it("隐式整体总结（无关键词）经 classifyModel 定 broad", async () => {
    const classifyModel = vi.fn(async () => classificationJson({
      intent: "whole_scope_analysis",
      coverageMode: "broad",
      scopeLevel: "notebook",
      confidence: 0.85,
      subQueries: ["核心思想", "理论体系脉络"],
    }));
    const plan = await planKnowledgeCoverage({
      question: "这本书的核心思想是什么？",
      turnScopeInfo: { notebookCount: 1, sourceCount: 2 },
      classifyModel: classifyModel as unknown as CoverageClassifyModel,
    });
    expect(plan.coverageMode).toBe("broad");
    expect(plan.classifierUsed).toBe("rules+llm"); // "是什么" 命中 fact 规则，语义层复核
    expect(plan.matchedRuleIds).toContain(RULE_FACT_LOOKUP);
    expect(plan.scopeLevel).toBe("notebook");
    expect(plan.subQueries).toEqual(["核心思想", "理论体系脉络"]);
    expect(classifyModel).toHaveBeenCalledTimes(1);
    expectOrthogonal(plan);
  });

  it("模型输出旧值 exhaustive → 归并 broad（存量/旧提示词习惯兼容）", async () => {
    const classifyModel = vi.fn(async () => classificationJson({
      coverageMode: "exhaustive",
      scopeLevel: "whole_scope",
      confidence: 0.95,
    }));
    const plan = await planKnowledgeCoverage({
      question: "这份报告整体有哪些重要风险？",
      classifyModel: classifyModel as unknown as CoverageClassifyModel,
    });
    expect(plan.coverageMode).toBe("broad");
    expectOrthogonal(plan);
  });

  it("whole_scope_analysis / global_negative 意图不允许低于 broad", async () => {
    const wholeScopePlan = await planKnowledgeCoverage({
      question: "这份报告的要点分布",
      classifyModel: (async () => classificationJson({
        intent: "whole_scope_analysis",
        coverageMode: "high_recall",
        confidence: 0.8,
      })) as unknown as CoverageClassifyModel,
    });
    expect(wholeScopePlan.coverageMode).toBe("broad");

    const globalNegativePlan = await planKnowledgeCoverage({
      question: "这份报告的要点分布",
      classifyModel: (async () => classificationJson({
        intent: "global_negative",
        coverageMode: "high_recall",
        confidence: 0.8,
      })) as unknown as CoverageClassifyModel,
    });
    expect(globalNegativePlan.coverageMode).toBe("broad");
  });

  it("open_summary + high_recall 维持 high_recall（不强升 broad）", async () => {
    const plan = await planKnowledgeCoverage({
      question: "帮我归纳这份材料",
      classifyModel: (async () => classificationJson({
        intent: "open_summary",
        coverageMode: "high_recall",
        confidence: 0.7,
      })) as unknown as CoverageClassifyModel,
    });
    expect(plan.coverageMode).toBe("high_recall");
  });

  it("首次输出非法 → 纠错重试一次；第二次合法则采用", async () => {
    const classifyModel = vi.fn()
      .mockResolvedValueOnce("这不是 JSON")
      .mockResolvedValueOnce(classificationJson({ intent: "open_summary", coverageMode: "broad", confidence: 0.7 }));
    const plan = await planKnowledgeCoverage({
      question: "帮我归纳这份材料",
      classifyModel: classifyModel as unknown as CoverageClassifyModel,
    });
    expect(plan.coverageMode).toBe("broad");
    expect(plan.classifierUsed).toBe("llm");
    expect(classifyModel).toHaveBeenCalledTimes(2);
    // 纠错重试携带上次错误与原始输出（复用 injector 的纠错模式）。
    const secondCall = (classifyModel.mock.calls[1] as unknown as Array<{
      correction?: { error: string; previousOutput: string };
    }>)[0];
    expect(secondCall.correction?.previousOutput).toBe("这不是 JSON");
  });

  it("LLM 连续输出非法 → 降级 high_recall 并留痕", async () => {
    const classifyModel = vi.fn(async () => "{invalid json");
    const plan = await planKnowledgeCoverage({
      question: "帮我归纳这份材料",
      classifyModel: classifyModel as unknown as CoverageClassifyModel,
    });
    expect(plan.coverageMode).toBe("high_recall");
    expect(plan.classifierUsed).toBe("rules");
    expect(plan.degradeReason).toBe("model output invalid after one correction retry");
    expect(classifyModel).toHaveBeenCalledTimes(2);
    expectOrthogonal(plan);
  });

  it("LLM 调用失败（超时/网络）→ 降级 high_recall 并留痕", async () => {
    const classifyModel = vi.fn(async () => {
      throw new Error("LLM_TIMEOUT");
    });
    const plan = await planKnowledgeCoverage({
      question: "这几份文件分别怎么看 X？",
      classifyModel: classifyModel as unknown as CoverageClassifyModel,
    });
    expect(plan.coverageMode).toBe("high_recall");
    expect(plan.degradeReason).toBe("model call failed");
    // 规则命中 id 照记（留痕判定来源）；范围提示不被降档失真。
    expect(plan.matchedRuleIds).toContain(RULE_MULTI_SOURCE);
    expect(plan.scopeLevel).toBe("multi_source");
    expectOrthogonal(plan);
  });

  it("classifyModel 收到 scope 元数据摘要", async () => {
    const classifyModel = vi.fn(async () => classificationJson({ coverageMode: "broad" }));
    await planKnowledgeCoverage({
      question: "归纳一下",
      turnScopeInfo: { notebookCount: 2, sourceCount: 5 },
      classifyModel: classifyModel as unknown as CoverageClassifyModel,
    });
    const firstCall = (classifyModel.mock.calls[0] as unknown as Array<{ scopeNote?: string }>)[0];
    expect(firstCall.scopeNote).toContain("2 notebook(s)");
    expect(firstCall.scopeNote).toContain("5 source(s)");
  });
});

describe("分类输出严格校验 parseCoverageClassification", () => {
  it("拒绝缺字段/多余字段/非法枚举/越界置信度/非法 subQueries", () => {
    expect(() => parseCoverageClassification("not json")).toThrow();
    expect(() => parseCoverageClassification('{"intent":"fact_lookup"}')).toThrow();
    expect(() => parseCoverageClassification(classificationJson({ coverageMode: "medium" }))).toThrow();
    expect(() => parseCoverageClassification(classificationJson({ scopeLevel: "everywhere" }))).toThrow();
    expect(() => parseCoverageClassification(classificationJson({ confidence: 1.5 }))).toThrow();
    // 两档化后 requiresCompleteness 是多余字段（schema 已移除）。
    expect(() => parseCoverageClassification(classificationJson({ requiresCompleteness: "yes" }))).toThrow();
    expect(() => parseCoverageClassification(classificationJson({ subQueries: ["a", "b", "c", "d", "e"] }))).toThrow();
    expect(() => parseCoverageClassification(classificationJson({ unknownField: 1 }))).toThrow();
    expect(parseCoverageClassification(classificationJson())).toMatchObject({ intent: "whole_scope_analysis" });
  });

  it("系统提示词为两档 schema、围栏禁令与禁 CoT 要求", () => {
    expect(KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT).toContain("high_recall");
    expect(KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT).toContain("broad");
    expect(KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT).toContain('\"coverageMode\":\"high_recall|broad\"');
    expect(KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT).toContain("There is no exhaustive mode");
    expect(KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT).not.toContain("requiresCompleteness");
    expect(KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT).toContain("Do not use Markdown fences");
    expect(KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT).toContain("Do not include reasoning");
  });
});

describe("持久化 round-trip（schema v13）", () => {
  function createStore() {
    const store = new KnowledgeStore({
      dbPath: path.join(tempHome("lingxi-coverage-store-"), "knowledge.db"),
      Database,
      now: () => "2026-08-29T00:00:00.000Z",
      idGenerator: (prefix) => `${prefix}_1`,
    });
    stores.push(store);
    return store;
  }

  function samplePlan(): KnowledgeCoveragePlan {
    return {
      intent: "cross_source_synthesis",
      coverageMode: "broad",
      scopeLevel: "multi_source",
      subQueries: ["利率风险", "信用风险"],
      confidence: 0.8,
      matchedRuleIds: [RULE_MULTI_SOURCE],
      classifierUsed: "rules",
    };
  }

  it("新库直接是最新 schema，表存在", () => {
    const store = createStore();
    expect(store.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);
    expect(store.db.prepare(
      "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'knowledge_coverage_plans'",
    ).get()).toBeTruthy();
  });

  it("insertCoveragePlan/getLatestCoveragePlan round-trip（含 scope 关联与全局最近）", () => {
    const store = createStore();
    const studioId = "studio-a";
    const notebook = store.createNotebook({ studioId, name: "NB" });
    store.createSourceWithSnapshot({
      studioId,
      notebookId: notebook.id,
      sourceId: "src_cov",
      sourceType: "pasted_text",
      displayName: "cov.txt",
      originMetadata: { kind: "pasted_text" },
      snapshot: {
        sha256: "a".repeat(64),
        mimeType: "text/plain",
        byteSize: 6,
        storagePath: "sources/src_cov/snap_cov.bin",
      },
    });
    const scope = store.createTurnScope({
      studioId,
      sessionPath: "/tmp/cov/session.jsonl",
      turnId: "turn_1",
      notebookIds: [notebook.id],
    });
    const record = store.insertCoveragePlan({
      turnScopeId: scope.id,
      question: "这几份文件分别如何看待利率风险？",
      plan: samplePlan(),
    });
    expect(record.id).toBeTruthy();
    expect(record.turnScopeId).toBe(scope.id);
    expect(record.coverageMode).toBe("broad");
    // 遗留列恒 false（exhaustive 档移除）。
    expect(record.requiresCompleteness).toBe(false);
    expect(record.subQueries).toEqual(["利率风险", "信用风险"]);
    expect(record.matchedRuleIds).toEqual([RULE_MULTI_SOURCE]);
    expect(record.classifierUsed).toBe("rules");
    expect(record.degradeReason).toBeNull();
    expect(record.createdAt).toBe("2026-08-29T00:00:00.000Z");

    // 按 scope 取最近 + 全局最近一致；无行的 scope 返回 null。
    expect(store.getLatestCoveragePlan({ turnScopeId: scope.id })).toEqual(record);
    expect(store.getLatestCoveragePlan()).toEqual(record);
    expect(store.getLatestCoveragePlan({ turnScopeId: "kts_missing" })).toBeNull();
  });

  it("降级计划（degradeReason）与 null turnScopeId round-trip", () => {
    const store = createStore();
    const record = store.insertCoveragePlan({
      turnScopeId: null,
      question: "帮我归纳这份材料",
      plan: {
        intent: "fact_lookup",
        coverageMode: "high_recall",
        scopeLevel: "source",
        confidence: 0.3,
        matchedRuleIds: [],
        classifierUsed: "rules",
        degradeReason: "model call failed",
      },
    });
    expect(record.turnScopeId).toBeNull();
    expect(record.degradeReason).toBe("model call failed");
    expect(record.subQueries).toEqual([]);
    expect(store.getLatestCoveragePlan()).toEqual(record);
  });

  it("非法入参显式拒绝；turnScopeId 必须指向存在的 scope", () => {
    const store = createStore();
    expect(() => store.insertCoveragePlan({ question: "", plan: samplePlan() })).toThrow();
    expect(() => store.insertCoveragePlan({
      question: "q",
      plan: { ...samplePlan(), coverageMode: "medium" },
    })).toThrow();
    expect(() => store.insertCoveragePlan({
      question: "q",
      turnScopeId: "kts_missing",
      plan: samplePlan(),
    })).toThrow(/turn scope/i);
  });

  it("KnowledgeManager 门面透传（engine 持久化路径）", async () => {
    const manager = new KnowledgeManager({ lingxiHome: tempHome("lingxi-coverage-manager-") });
    managers.push(manager);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "NB" });
    await manager.importPastedText({ studioId, notebookId: notebook.id, text: "内容", displayName: "cov.txt" });
    const scope = manager.createTurnScope({
      studioId,
      sessionPath: "/tmp/cov-mgr/session.jsonl",
      turnId: "turn_1",
      notebookIds: [notebook.id],
    });
    const plan = await planKnowledgeCoverage({ question: "全文提到的风险都列出来" });
    const record = manager.insertCoveragePlan({
      turnScopeId: scope.id,
      question: "全文提到的风险都列出来",
      plan,
    });
    expect(record.coverageMode).toBe("broad");
    expect(manager.getLatestCoveragePlan({ turnScopeId: scope.id })).toEqual(record);
  });
});

describe("injector 集成回归（Phase 7：只标注，不改变检索行为）", () => {
  function fakeChunk(id: string, ordinal: number) {
    return {
      id,
      parseArtifactId: "parse-1",
      ordinal,
      text: `证据文本 ${id}`,
      tokenCount: 6,
      spans: [],
      score: 1,
      notebookId: "nb-1",
      notebookName: "资料",
      sourceId: "src-1",
      sourceName: "源",
      headingPath: null,
      pageNumber: null,
    } as RetrieveForNotebooksResult["candidates"][number];
  }

  function fakeRetrieval(candidates: RetrieveForNotebooksResult["candidates"]): RetrieveForNotebooksResult {
    return { candidates, sources: [], retrievalMode: "fts", retrievalModeRequested: "fts", degraded: [] };
  }

  function buildDeps(retrieveQueries: string[]) {
    const chunks = [fakeChunk("chunk_a", 0), fakeChunk("chunk_b", 1)];
    return {
      decomposeModel: null,
      rollupModel: null,
      retrieve: ({ query }: { query: string }) => {
        retrieveQueries.push(query);
        return Promise.resolve(fakeRetrieval(chunks));
      },
    };
  }

  const broadPlan: KnowledgeCoveragePlan = {
    intent: "cross_source_synthesis",
    coverageMode: "broad",
    scopeLevel: "multi_source",
    confidence: 0.8,
    matchedRuleIds: [RULE_MULTI_SOURCE],
    classifierUsed: "rules",
  };

  it("注入块头出现 coverage 标注、stats 字段透出", async () => {
    const queries: string[] = [];
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "这几份文件分别如何看待 X？",
      mode: "detailed",
      deps: buildDeps(queries),
      budgetTokens: 10_000,
      coveragePlan: broadPlan,
    });
    expect(block).toContain("[coverage: broad · multi_source]");
    expect(stats.coverageMode).toBe("broad");
    expect(stats.scopeLevel).toBe("multi_source");
    expect(stats.matchedRuleIds).toEqual([RULE_MULTI_SOURCE]);
  });

  it("降级计划的块头标注携带降级原因（显式留痕）", async () => {
    const queries: string[] = [];
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "归纳一下",
      mode: "detailed",
      deps: buildDeps(queries),
      budgetTokens: 10_000,
      coveragePlan: { ...broadPlan, coverageMode: "high_recall", scopeLevel: "source", degradeReason: "model call failed" },
    });
    expect(block).toContain("[coverage: high_recall · source] (coverage classifier degraded: model call failed)");
    expect(stats.coverageMode).toBe("high_recall");
  });

  it("coveragePlan 以 Promise 传入：直检先行，planner 先于拆解落定", async () => {
    const queries: string[] = [];
    const events: string[] = [];
    const deps = buildDeps(queries);
    const planPromise = (async () => {
      events.push("plan-resolved");
      return broadPlan;
    })();
    // 直检在 injector 内同步启动：promise 微任务 resolve 前 retrieve 已发起。
    const injectionPromise = buildKnowledgeContextInjection({
      question: "这几份文件分别如何看待 X？",
      mode: "detailed",
      deps,
      budgetTokens: 10_000,
      coveragePlan: planPromise,
    });
    expect(queries).toEqual(["这几份文件分别如何看待 X？"]);
    const { block } = await injectionPromise;
    expect(events).toEqual(["plan-resolved"]);
    expect(block).toContain("[coverage: broad · multi_source]");
  });

  it("检索行为与无 planner 时一致：stats 除 coverage 字段外全等、块内差异仅标注一行", async () => {
    const run = async (coveragePlan?: KnowledgeCoveragePlan) => {
      const queries: string[] = [];
      const { block, stats } = await buildKnowledgeContextInjection({
        question: "这几份文件分别如何看待 X？",
        mode: "detailed",
        deps: buildDeps(queries),
        budgetTokens: 10_000,
        ...(coveragePlan ? { coveragePlan } : {}),
      });
      return { block, stats, queries };
    };
    const without = await run();
    const withPlan = await run(broadPlan);
    // 检索调用与候选/注入结果零变化。
    expect(withPlan.queries).toEqual(without.queries);
    expect(withPlan.stats.fusedChunks).toBe(without.stats.fusedChunks);
    expect(withPlan.stats.injectedChunks).toBe(without.stats.injectedChunks);
    expect(withPlan.stats.results).toEqual(without.stats.results);
    expect(withPlan.stats.subQueries).toEqual(without.stats.subQueries);
    expect(withPlan.stats.usedTokens).toBe(without.stats.usedTokens);
    // 块内差异 = 恰好一行 coverage 标注。
    const withoutLines = without.block.split("\n");
    const withLines = withPlan.block.split("\n");
    const coverageLineIndex = withLines.indexOf("[coverage: broad · multi_source]");
    expect(coverageLineIndex).toBeGreaterThan(-1);
    expect(withLines.filter(line => line !== "[coverage: broad · multi_source]")).toEqual(withoutLines);
  });

  it("未传 coveragePlan（旧路径）：无 coverage 标注行、stats 无 coverage 字段", async () => {
    const queries: string[] = [];
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "这几份文件分别如何看待 X？",
      mode: "detailed",
      deps: buildDeps(queries),
      budgetTokens: 10_000,
    });
    expect(block).not.toContain("[coverage:");
    expect(stats).not.toHaveProperty("coverageMode");
    expect(stats).not.toHaveProperty("scopeLevel");
    expect(stats).not.toHaveProperty("requiresCompleteness");
    expect(stats).not.toHaveProperty("matchedRuleIds");
  });
});
