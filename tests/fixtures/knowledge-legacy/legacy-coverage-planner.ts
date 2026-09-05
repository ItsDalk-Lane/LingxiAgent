/** 已退役的覆盖规划回归夹具，不进入生产运行闭包。 */
import { KnowledgeError } from "../../../lib/knowledge/errors.ts";
import type { KnowledgeCoverageMode, KnowledgeCoverageScopeLevel } from "../../../shared/knowledge-refs.ts";


/**
 * 规则 id（§三十一；稳定枚举，持久化进 matched_rule_ids_json）。前两个是
 * exhaustive 时代的定档规则，两档化后命中改定 broad——id 字符串不动，存量行
 * 留痕可读。
 */
export const RULE_EXHAUSTIVE_KEYWORD = "RULE_EXHAUSTIVE_KEYWORD";

export const RULE_GLOBAL_NEGATIVE = "RULE_GLOBAL_NEGATIVE";

export const RULE_MULTI_SOURCE = "RULE_MULTI_SOURCE";

export const RULE_FACT_LOOKUP = "RULE_FACT_LOOKUP";


/**
 * §三十一 全词表：以下词义强烈指示全库/完整性诉求（历史上的 exhaustive 触发
 * 词）。两档化后命中定 broad——整库阅读需求由 injector 的滚动多轮注入在执行侧
 * 承担。中文按子串匹配（这些词在正常问句中不构成歧义子串）；"等"类扩展只在
 * 任务书词表内取词，不做自由发挥。
 */
const EXHAUSTIVE_KEYWORDS: readonly string[] = [
  "全文", "全书", "整本", "整篇", "全部", "所有",
  "每一章", "每一个", "逐章", "逐节",
  "完整梳理", "全面分析", "从头到尾", "通篇", "贯穿全文",
  "不要遗漏", "有没有遗漏", "列出所有", "所有出现", "所有提到",
];


/**
 * global-negative / 完整性证明句式（§三十一："所有X是否"按 X 为 ≤12 个非停顿
 * 字符的短段匹配，覆盖"所有出现的X是否……"一类嵌套）。
 */
const GLOBAL_NEGATIVE_PATTERNS: readonly RegExp[] = [
  /有没有任何/,
  /是否存在任何/,
  /有没有反例/,
  /是否没有/,
  /全文是否/,
  /所有[^\s，。？！；、,?!;]{0,12}是否/,
];


/** 多源指代（§三十"这几份材料如何看待 X"）。 */
const MULTI_SOURCE_KEYWORDS: readonly string[] = ["这几份", "这些文件", "这些资料", "这几篇", "分别"];


/** 单点事实（§三十三：普通事实问题默认 high_recall）。 */
const FACT_LOOKUP_KEYWORDS: readonly string[] = ["何时", "什么时候", "什么是", "是什么", "什么叫", "定义", "哪一年"];


/**
 * 语义层判定超时纪律与拆解层同源（首次 15s / 纠错重试 8s）由 classifyModel
 * 闭包（engine 侧 callText 封装）负责，本模块只做编排与校验。
 */
export type CoverageClassifyModel = (input: {
  question: string;
  /** scope 元数据摘要（选中笔记本数/源数）；未知时缺省。 */
  scopeNote?: string;
  correction?: { error: string; previousOutput: string };
}) => Promise<string>;


/**
 * turnScope 元数据（不要求完整 scope 对象）：选中 notebook 数与本轮冻结的
 * 去重源数，供 scopeLevel 判定（§三十）。null = 未知（非会话路径）。
 */
export interface KnowledgeCoverageTurnScopeInfo {
  notebookCount?: number | null;
  sourceCount?: number | null;
}


/** LLM 分类输出（严格 JSON schema 的解析结果）。 */
export interface CoverageClassification {
  intent: KnowledgeCoverageIntent;
  coverageMode: KnowledgeCoverageMode;
  scopeLevel: KnowledgeCoverageScopeLevel;
  confidence: number;
  subQueries?: string[];
}


const CLASSIFY_OUTPUT_MAX_CHARS = 10_000;

const CLASSIFY_SUBQUERY_MAX = 4;

const CLASSIFY_SUBQUERY_MAX_CHARS = 500;


/**
 * 语义分类系统提示词。规则风格对齐 KNOWLEDGE_DECOMPOSE_SYSTEM_PROMPT
 * （编号规则 + 严格 JSON schema + 禁 Markdown 围栏）。
 */
export const KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT = `You classify a user question to plan knowledge retrieval coverage.

Rules:
1. intent: fact_lookup (one focused fact), cross_source_synthesis (compare or combine several sources), whole_scope_analysis (the whole selected scope such as a book or report as a unit), global_negative (proving absence or checking every occurrence), or open_summary (open-ended summary of the content).
2. coverageMode: high_recall (focused retrieval for a specific point) or broad (deep retrieval emphasizing source/section structure coverage). Whole-scope questions and completeness-heavy questions take broad; focused lookups take high_recall. There is no exhaustive mode.
3. scopeLevel: local (inside one section), source (one document), multi_source (several documents), notebook (one notebook as a unit), multi_notebook (several notebooks), whole_scope (everything selected this turn). Use the scope metadata in the user message when present.
4. subQueries (optional, 0 to 4): narrow retrieval queries for the facets that need coverage. Keep proper nouns exactly as written. Never embed instructions for the reader inside a sub-query.
5. Return one JSON object and nothing else. Do not use Markdown fences. Do not include reasoning.

Schema:
{"intent":"fact_lookup|cross_source_synthesis|whole_scope_analysis|global_negative|open_summary","coverageMode":"high_recall|broad","scopeLevel":"local|source|multi_source|notebook|multi_notebook|whole_scope","subQueries":["..."],"confidence":0.0}`;


/**
 * 解析并严格校验语义分类输出（requiredObject 风格）：纯 JSON、5 个必备字段
 * 精确校验（枚举/布尔/0-1 置信度）+ 可选 subQueries（0-4 条非空 ≤500 字符、
 * 去重）。任何不符抛 KNOWLEDGE_MODEL_OUTPUT_INVALID。
 */
export function parseCoverageClassification(raw: string): CoverageClassification {
  if (typeof raw !== "string" || !raw.trim() || raw.length > CLASSIFY_OUTPUT_MAX_CHARS) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification output is empty or too large");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification output is not valid JSON");
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification output must be an object");
  }
  const record = parsed as Record<string, unknown>;
  for (const key of ["intent", "coverageMode", "scopeLevel", "confidence"]) {
    if (!Object.hasOwn(record, key)) {
      throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", `Coverage classification field '${key}' is missing`);
    }
  }
  const extraKeys = Object.keys(record).filter(key => key !== "subQueries"
    && !["intent", "coverageMode", "scopeLevel", "confidence"].includes(key));
  if (extraKeys.length > 0) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification output fields are invalid");
  }
  if (!isKnowledgeCoverageIntent(record.intent)) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification intent is invalid");
  }
  if (!isKnowledgeCoverageMode(record.coverageMode)) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification coverageMode is invalid");
  }
  if (!isKnowledgeCoverageScopeLevel(record.scopeLevel)) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification scopeLevel is invalid");
  }
  const confidence = Number(record.confidence);
  if (!Number.isFinite(confidence) || confidence < 0 || confidence > 1) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification confidence must be within 0 and 1");
  }
  const classification: CoverageClassification = {
    intent: record.intent,
    coverageMode: record.coverageMode,
    scopeLevel: record.scopeLevel,
    confidence,
  };
  if (Object.hasOwn(record, "subQueries")) {
    if (!Array.isArray(record.subQueries)) {
      throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification subQueries must be an array");
    }
    if (record.subQueries.length > CLASSIFY_SUBQUERY_MAX) {
      throw new KnowledgeError(
        "KNOWLEDGE_MODEL_OUTPUT_INVALID",
        `Coverage classification must contain at most ${CLASSIFY_SUBQUERY_MAX} sub-queries`,
      );
    }
    const seen = new Set<string>();
    const subQueries: string[] = [];
    for (const rawQuery of record.subQueries) {
      if (typeof rawQuery !== "string") {
        throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification sub-query must be a string");
      }
      const trimmed = rawQuery.trim();
      if (!trimmed || trimmed.length > CLASSIFY_SUBQUERY_MAX_CHARS) {
        throw new KnowledgeError(
          "KNOWLEDGE_MODEL_OUTPUT_INVALID",
          `Coverage classification sub-query must be non-empty and at most ${CLASSIFY_SUBQUERY_MAX_CHARS} characters`,
        );
      }
      if (seen.has(trimmed)) continue;
      seen.add(trimmed);
      subQueries.push(trimmed);
    }
    if (subQueries.length > 0) classification.subQueries = subQueries;
  }
  return classification;
}


/** 规则层评估结果：matchedRuleIds 按判定顺序。 */
export interface CoverageRuleMatch {
  matchedRuleIds: string[];
  /** 规则层给出的档位；null = 未定档（交给语义层或规则默认）。 */
  coverageMode: KnowledgeCoverageMode | null;
  intent: KnowledgeCoverageIntent | null;
}


/**
 * 第一层确定性规则（§三十一 全词表，两档化改写）。判定顺序：全库/完整性
 * 关键词 → global-negative 句式（两者可叠加命中，均定 broad）→ 多源指代 →
 * 单点事实。规则只给默认档，classifyModel 可用时仍交语义层复核。
 */
export function matchCoverageRules(question: string): CoverageRuleMatch {
  const matchedRuleIds: string[] = [];
  let coverageMode: KnowledgeCoverageMode | null = null;
  let intent: KnowledgeCoverageIntent | null = null;
  if (EXHAUSTIVE_KEYWORDS.some(keyword => question.includes(keyword))) {
    matchedRuleIds.push(RULE_EXHAUSTIVE_KEYWORD);
    coverageMode = "broad";
    intent = "whole_scope_analysis";
  }
  if (GLOBAL_NEGATIVE_PATTERNS.some(pattern => pattern.test(question))) {
    matchedRuleIds.push(RULE_GLOBAL_NEGATIVE);
    coverageMode = "broad";
    intent = "global_negative";
  }
  if (MULTI_SOURCE_KEYWORDS.some(keyword => question.includes(keyword))) {
    matchedRuleIds.push(RULE_MULTI_SOURCE);
    coverageMode = "broad";
    intent = "cross_source_synthesis";
  } else if (FACT_LOOKUP_KEYWORDS.some(keyword => question.includes(keyword))) {
    matchedRuleIds.push(RULE_FACT_LOOKUP);
    coverageMode = "high_recall";
    intent = "fact_lookup";
  }
  return { matchedRuleIds, coverageMode: matchedRuleIds.length > 0 ? coverageMode : null, intent };
}


/**
 * scopeLevel 的元数据推导（§三十）：选中集合的规模决定层级上限，未知元数据
 * 按本轮全量口径（whole_scope，宁大勿小）。语义层可给出更精确判定。
 */
export function coverageScopeLevelFromMetadata(
  info: KnowledgeCoverageTurnScopeInfo | null | undefined,
): KnowledgeCoverageScopeLevel {
  const notebookCount = info?.notebookCount ?? null;
  const sourceCount = info?.sourceCount ?? null;
  if (notebookCount != null && notebookCount > 1) return "multi_notebook";
  if (notebookCount === 1) {
    if (sourceCount != null && sourceCount > 1) return "multi_source";
    if (sourceCount === 1) return "source";
    return "notebook";
  }
  return "whole_scope";
}


function describeError(error: unknown): string {
  if (error instanceof KnowledgeError) return `${error.code}: ${error.message}`;
  if (error instanceof Error) return error.message;
  return String(error);
}


function scopeNoteOf(info: KnowledgeCoverageTurnScopeInfo | null | undefined): string | undefined {
  const notebookCount = info?.notebookCount ?? null;
  const sourceCount = info?.sourceCount ?? null;
  if (notebookCount == null && sourceCount == null) return undefined;
  const parts: string[] = [];
  if (notebookCount != null) parts.push(`${notebookCount} notebook(s)`);
  if (sourceCount != null) parts.push(`${sourceCount} source(s)`);
  return `Selected knowledge scope: ${parts.join(", ")}.`;
}


/**
 * 规则层终稿（无模型路径共用）。规则未命中时按保守默认 high_recall
 * （镜像当前检索姿态），留痕场合由调用方补 degradeReason。
 */
/**
 * scopeLevel 推导（规则层共用）：元数据为准；多源指代规则（"这几份"类）是
 * 确定性事实，元数据低于 multi_source（含未知元数据的 whole_scope 缺省）时按
 * multi_source 记（与语义层归并同纪律）。
 */
function ruleScopeLevel(
  rules: CoverageRuleMatch,
  info: KnowledgeCoverageTurnScopeInfo | null | undefined,
): KnowledgeCoverageScopeLevel {
  const metadataScope = coverageScopeLevelFromMetadata(info);
  if (rules.matchedRuleIds.includes(RULE_MULTI_SOURCE)
    && metadataScope !== "multi_source"
    && metadataScope !== "multi_notebook") {
    return "multi_source";
  }
  return metadataScope;
}


function rulesOnlyPlan(input: {
  rules: CoverageRuleMatch;
  info: KnowledgeCoverageTurnScopeInfo | null | undefined;
  degradeReason?: CoverageDegradeReason;
}): KnowledgeCoveragePlan {
  if (input.rules.coverageMode === "broad") {
    // 两档化：broad 吸收全库/完整性问题后不再一律下限 multi_source——scopeLevel
    // 如实按元数据推导（单源整书 = source，单笔记本 = notebook），多源指代
    // 规则命中时保留 multi_source 下限。
    return {
      intent: input.rules.intent ?? "cross_source_synthesis",
      coverageMode: "broad",
      scopeLevel: ruleScopeLevel(input.rules, input.info),
      confidence: 0.8,
      matchedRuleIds: [...input.rules.matchedRuleIds],
      classifierUsed: "rules",
    };
  }
  if (input.rules.coverageMode === "high_recall") {
    return {
      intent: input.rules.intent ?? "fact_lookup",
      coverageMode: "high_recall",
      scopeLevel: "local",
      confidence: 0.75,
      matchedRuleIds: [...input.rules.matchedRuleIds],
      classifierUsed: "rules",
    };
  }
  // 规则未定档：保守默认 high_recall（当前检索姿态），低置信 + 降级留痕。
  return {
    intent: "fact_lookup",
    coverageMode: "high_recall",
    scopeLevel: ruleScopeLevel(input.rules, input.info),
    confidence: 0.5,
    matchedRuleIds: [...input.rules.matchedRuleIds],
    classifierUsed: "rules",
    ...(input.degradeReason ? { degradeReason: input.degradeReason } : {}),
  };
}


/**
 * 语义层失败后的统一降级终稿（调用失败 / 连续输出无效）：按任务书规定降
 * high_recall 并留稳定 degradeReason；规则命中 id 照记（留痕说明判定来源），
 * scopeLevel 保留规则/元数据推导的范围提示（范围描述与档位正交，不随降档失真）。
 */
function degradedAfterClassifierFailure(input: {
  rules: CoverageRuleMatch;
  info: KnowledgeCoverageTurnScopeInfo | null | undefined;
  reason: CoverageDegradeReason;
}): KnowledgeCoveragePlan {
  // 范围提示与档位正交：降级留痕时不随降档失真（多源指代命中保留 multi_source）。
  const scopeLevel = ruleScopeLevel(input.rules, input.info);
  return {
    intent: "fact_lookup",
    coverageMode: "high_recall",
    scopeLevel,
    confidence: 0.3,
    matchedRuleIds: [...input.rules.matchedRuleIds],
    classifierUsed: "rules",
    degradeReason: input.reason,
  };
}


/**
 * 语义层判定归并（两档化）：模型/存量旧值输出 'exhaustive' 一律映射 broad；
 * whole_scope_analysis / global_negative 意图蕴含强覆盖诉求，不允许低于 broad。
 */
function mergeClassification(input: {
  rules: CoverageRuleMatch;
  info: KnowledgeCoverageTurnScopeInfo | null | undefined;
  classification: CoverageClassification;
}): KnowledgeCoveragePlan {
  const { classification } = input;
  let coverageMode: KnowledgeCoverageMode = classification.coverageMode === "exhaustive"
    ? "broad"
    : classification.coverageMode;
  if ((classification.intent === "whole_scope_analysis" || classification.intent === "global_negative")
    && coverageMode === "high_recall") {
    coverageMode = "broad";
  }
  // 多源指代规则是确定性事实：语义层的范围判定不能低于 multi_source。
  let scopeLevel = classification.scopeLevel;
  if (input.rules.matchedRuleIds.includes(RULE_MULTI_SOURCE)
    && (scopeLevel === "local" || scopeLevel === "source" || scopeLevel === "notebook")) {
    scopeLevel = "multi_source";
  }
  return {
    intent: classification.intent,
    coverageMode,
    scopeLevel,
    ...(classification.subQueries && classification.subQueries.length > 0
      ? { subQueries: [...classification.subQueries] }
      : {}),
    confidence: classification.confidence,
    matchedRuleIds: [...input.rules.matchedRuleIds],
    classifierUsed: input.rules.matchedRuleIds.length > 0 ? "rules+llm" : "llm",
  };
}


/**
 * 覆盖计划主入口（§二十九概念接口的落地形态：turnScopeInfo 只取元数据，
 * 不要求完整 scope 对象）。总函数：任何模型失败/输出非法都在 plan 内显式
 * 降级留痕，不抛错、不阻断注入（对齐拆解层降级纪律）。
 *
 * - classifyModel 可用 → 一次严格 JSON 分类（纠错重试一次），连续无效/调用
 *   失败 → 降级 high_recall 并留 degradeReason；
 * - 无 classifyModel → 规则结果即终稿；规则也未定档时按保守默认 high_recall
 *   并留 "knowledge model slot not configured" 痕。
 */
export async function planKnowledgeCoverage(input: {
  question: string;
  turnScopeInfo?: KnowledgeCoverageTurnScopeInfo | null;
  classifyModel?: CoverageClassifyModel | null;
}): Promise<KnowledgeCoveragePlan> {
  const question = input.question.trim();
  const rules = matchCoverageRules(question);
  if (!input.classifyModel) {
    // 无模型：规则结果即终稿；规则也未定档 → 保守默认 + 显式留痕。
    return rulesOnlyPlan({
      rules,
      info: input.turnScopeInfo,
      ...(rules.coverageMode == null ? { degradeReason: "knowledge model slot not configured" as const } : {}),
    });
  }
  const scopeNote = scopeNoteOf(input.turnScopeInfo);
  let firstError = "";
  let firstOutput = "";
  for (let attempt = 0; attempt < 2; attempt += 1) {
    let raw: string;
    try {
      raw = await input.classifyModel(attempt === 0
        ? { question, ...(scopeNote ? { scopeNote } : {}) }
        : { question, ...(scopeNote ? { scopeNote } : {}), correction: { error: firstError, previousOutput: firstOutput } });
    } catch {
      // 调用失败（超时/网络）：降级 high_recall 并留痕；明细由调用方日志承载。
      return degradedAfterClassifierFailure({
        rules,
        info: input.turnScopeInfo,
        reason: "model call failed",
      });
    }
    try {
      const classification = parseCoverageClassification(raw);
      return mergeClassification({ rules, info: input.turnScopeInfo, classification });
    } catch (error) {
      if (attempt === 0) {
        firstError = describeError(error);
        firstOutput = raw.slice(0, 2000);
        continue;
      }
      // 连续无效：降级 high_recall 并留痕（任务书规定的保守档）。
      return degradedAfterClassifierFailure({
        rules,
        info: input.turnScopeInfo,
        reason: "model output invalid after one correction retry",
      });
    }
  }
  // 循环必然 return；此处仅为类型完备。
  return rulesOnlyPlan({ rules, info: input.turnScopeInfo });
}

import { type KnowledgeCoverageIntent, type CoverageDegradeReason, type KnowledgeCoveragePlan, isKnowledgeCoverageIntent, isKnowledgeCoverageMode, isKnowledgeCoverageScopeLevel } from "../../../lib/knowledge/knowledge-coverage-planner.ts";
export { type KnowledgeCoverageIntent, type KnowledgeCoverageClassifierUsed, type CoverageDegradeReason, type KnowledgeCoveragePlan, type KnowledgeCoveragePlanRecord, isKnowledgeCoverageIntent, isKnowledgeCoverageMode, isKnowledgeCoverageScopeLevel, isKnowledgeCoverageClassifierUsed } from "../../../lib/knowledge/knowledge-coverage-planner.ts";
