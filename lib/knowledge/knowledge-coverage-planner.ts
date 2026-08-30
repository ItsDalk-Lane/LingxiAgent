/**
 * knowledge-coverage-planner —— 覆盖策略规划（任务书 §二十七–§三十二/§四十一，Phase 7）。
 *
 * 三维度正交（§二十八）：coverageMode（high_recall/broad/exhaustive）与 answerMode
 * （qa/assist）、retrievalMode（fts/hybrid）互不携带、互不影响——plan 不包含也
 * 不读这两个维度；执行侧自 Phase 8 起在 injector 消费（按档位切换检索行为），
 * 本模块只负责把判定做对。
 *
 * 两层判定：
 * 1. 确定性规则层（§三十一）：exhaustive 关键词与 global-negative 句式直接定档
 *    exhaustive（§四十一：明确触发不浪费一次普通检索，不依赖 LLM）；多源指代 →
 *    broad；单点事实 → high_recall。规则未定档的部分交给第二层。
 * 2. 语义判断层（§三十二）：classifyModel 可用时用一次严格 JSON 输出的 LLM 分类
 *    （复用 injector 的纠错重试模式：输出 schema 校验失败重试一次，再失败降级
 *    high_recall 并留 degradeReason）；无 classifyModel → 规则结果即终稿。
 *
 * 不变量：requiresCompleteness ⟺ coverageMode === "exhaustive"（exhaustive 的
 * 定义就是"确定性扫描全部可处理内容"，完整性义务与档位一体两面；§三十一明确
 * 这类问题不能靠 TopK Search 证明）。
 *
 * 禁 CoT（§二十九）：模型输出只取结构化分类结果，reasoning 一律不落库不透传。
 * 纯函数化可测：模型调用依赖注入，本模块不做 IO。
 */
import { KnowledgeError } from "./errors.ts";
import type { KnowledgeCoverageMode, KnowledgeCoverageScopeLevel } from "../../shared/knowledge-refs.ts";

export type { KnowledgeCoverageMode, KnowledgeCoverageScopeLevel } from "../../shared/knowledge-refs.ts";

/** 问题意图（§三十/scopeLevel 语义配套；与拆解层的 QuestionIntent 是两套口径）。 */
export type KnowledgeCoverageIntent =
  | "fact_lookup"
  | "cross_source_synthesis"
  | "whole_scope_analysis"
  | "global_negative"
  | "open_summary";

/** 终稿由哪一层产出。 */
export type KnowledgeCoverageClassifierUsed = "rules" | "llm" | "rules+llm";

/** 语义层降级原因（与拆解层 DecomposeDegradeReason 同风格的稳定枚举）。 */
export type CoverageDegradeReason =
  | "knowledge model slot not configured"
  | "model output invalid after one correction retry"
  | "model call failed";

/**
 * 覆盖计划（§二十九）：只含结构化判定结果。不携带 answerMode/retrievalMode
 * （三维度正交），不含任何模型推理文本。
 */
export interface KnowledgeCoveragePlan {
  intent: KnowledgeCoverageIntent;
  coverageMode: KnowledgeCoverageMode;
  requiresCompleteness: boolean;
  scopeLevel: KnowledgeCoverageScopeLevel;
  /** 可选的覆盖面检索子查询（语义层产出，0-4 条；执行侧按需消费）。 */
  subQueries?: string[];
  /** 0-1；规则层定档为确定性高置信。 */
  confidence: number;
  /** 命中的确定性规则 id（按判定顺序）。 */
  matchedRuleIds: string[];
  classifierUsed: KnowledgeCoverageClassifierUsed;
  /** 语义层不可用/失败时的降级留痕（禁静默降级红线）。 */
  degradeReason?: string;
}

/** knowledge_coverage_plans 表的读取形状（schema v13；question/归属列随行）。 */
export interface KnowledgeCoveragePlanRecord {
  id: string;
  turnScopeId: string | null;
  question: string;
  createdAt: string;
  intent: KnowledgeCoverageIntent;
  coverageMode: KnowledgeCoverageMode;
  requiresCompleteness: boolean;
  scopeLevel: KnowledgeCoverageScopeLevel;
  subQueries: string[];
  confidence: number;
  matchedRuleIds: string[];
  classifierUsed: KnowledgeCoverageClassifierUsed;
  /** 持久化列可空归一（plan 里的可选字段在行里是 null）。 */
  degradeReason: string | null;
}

/** 规则 id（§三十一；稳定枚举，持久化进 matched_rule_ids_json）。 */
export const RULE_EXHAUSTIVE_KEYWORD = "RULE_EXHAUSTIVE_KEYWORD";
export const RULE_GLOBAL_NEGATIVE = "RULE_GLOBAL_NEGATIVE";
export const RULE_MULTI_SOURCE = "RULE_MULTI_SOURCE";
export const RULE_FACT_LOOKUP = "RULE_FACT_LOOKUP";

/**
 * §三十一 全词表：以下词义强烈触发 exhaustive。中文按子串匹配（这些词在
 * 正常问句中不构成歧义子串）；"等"类扩展只在任务书词表内取词，不做自由发挥。
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

const COVERAGE_INTENTS = new Set<KnowledgeCoverageIntent>([
  "fact_lookup",
  "cross_source_synthesis",
  "whole_scope_analysis",
  "global_negative",
  "open_summary",
]);
const COVERAGE_MODES = new Set<KnowledgeCoverageMode>(["high_recall", "broad", "exhaustive"]);
const COVERAGE_SCOPE_LEVELS = new Set<KnowledgeCoverageScopeLevel>([
  "local",
  "source",
  "multi_source",
  "notebook",
  "multi_notebook",
  "whole_scope",
]);
const COVERAGE_CLASSIFIERS = new Set<KnowledgeCoverageClassifierUsed>(["rules", "llm", "rules+llm"]);

export function isKnowledgeCoverageIntent(value: unknown): value is KnowledgeCoverageIntent {
  return typeof value === "string" && (COVERAGE_INTENTS as Set<string>).has(value);
}

export function isKnowledgeCoverageMode(value: unknown): value is KnowledgeCoverageMode {
  return typeof value === "string" && (COVERAGE_MODES as Set<string>).has(value);
}

export function isKnowledgeCoverageScopeLevel(value: unknown): value is KnowledgeCoverageScopeLevel {
  return typeof value === "string" && (COVERAGE_SCOPE_LEVELS as Set<string>).has(value);
}

export function isKnowledgeCoverageClassifierUsed(value: unknown): value is KnowledgeCoverageClassifierUsed {
  return typeof value === "string" && (COVERAGE_CLASSIFIERS as Set<string>).has(value);
}

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
  requiresCompleteness: boolean;
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
2. coverageMode: high_recall (find as much relevant content as possible), broad (emphasize source/section structure coverage), exhaustive (deterministic scan of all processable content).
3. requiresCompleteness is true only when a correct answer depends on covering every matching passage (completeness obligation). Such questions cannot be proven by top-K search.
4. Whole-scope questions without explicit keywords ("core idea of this book", "overall theory", "key stages", "overall risks") are exhaustive when you are confident, broad when less confident; set confidence honestly between 0 and 1.
5. scopeLevel: local (inside one section), source (one document), multi_source (several documents), notebook (one notebook as a unit), multi_notebook (several notebooks), whole_scope (everything selected this turn). Use the scope metadata in the user message when present.
6. subQueries (optional, 0 to 4): narrow retrieval queries for the facets that need coverage. Keep proper nouns exactly as written. Never embed instructions for the reader inside a sub-query.
7. Return one JSON object and nothing else. Do not use Markdown fences. Do not include reasoning.

Schema:
{"intent":"fact_lookup|cross_source_synthesis|whole_scope_analysis|global_negative|open_summary","coverageMode":"high_recall|broad|exhaustive","requiresCompleteness":false,"scopeLevel":"local|source|multi_source|notebook|multi_notebook|whole_scope","subQueries":["..."],"confidence":0.0}`;

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
  for (const key of ["intent", "coverageMode", "requiresCompleteness", "scopeLevel", "confidence"]) {
    if (!Object.hasOwn(record, key)) {
      throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", `Coverage classification field '${key}' is missing`);
    }
  }
  const extraKeys = Object.keys(record).filter(key => key !== "subQueries"
    && !["intent", "coverageMode", "requiresCompleteness", "scopeLevel", "confidence"].includes(key));
  if (extraKeys.length > 0) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification output fields are invalid");
  }
  if (!isKnowledgeCoverageIntent(record.intent)) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification intent is invalid");
  }
  if (!isKnowledgeCoverageMode(record.coverageMode)) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification coverageMode is invalid");
  }
  if (typeof record.requiresCompleteness !== "boolean") {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Coverage classification requiresCompleteness must be a boolean");
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
    requiresCompleteness: record.requiresCompleteness,
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

/** 规则层评估结果：matchedRuleIds 按判定顺序；definitive = 直接定档（跳过 LLM）。 */
export interface CoverageRuleMatch {
  matchedRuleIds: string[];
  /** 规则层给出的档位；null = 未定档（交给语义层或规则默认）。 */
  coverageMode: KnowledgeCoverageMode | null;
  intent: KnowledgeCoverageIntent | null;
  /** §四十一：exhaustive 规则命中即终稿，不再询问 LLM。 */
  definitive: boolean;
}

/**
 * 第一层确定性规则（§三十一 全词表）。判定顺序：exhaustive 关键词 →
 * global-negative 句式（两者可叠加命中）→ 多源指代 → 单点事实。只有
 * exhaustive/global-negative 是定档规则；broad/high_recall 是规则默认档，
 * classifyModel 可用时仍交语义层复核升级。
 */
export function matchCoverageRules(question: string): CoverageRuleMatch {
  const matchedRuleIds: string[] = [];
  let coverageMode: KnowledgeCoverageMode | null = null;
  let intent: KnowledgeCoverageIntent | null = null;
  let definitive = false;
  if (EXHAUSTIVE_KEYWORDS.some(keyword => question.includes(keyword))) {
    matchedRuleIds.push(RULE_EXHAUSTIVE_KEYWORD);
    coverageMode = "exhaustive";
    intent = "whole_scope_analysis";
    definitive = true;
  }
  if (GLOBAL_NEGATIVE_PATTERNS.some(pattern => pattern.test(question))) {
    matchedRuleIds.push(RULE_GLOBAL_NEGATIVE);
    coverageMode = "exhaustive";
    intent = "global_negative";
    definitive = true;
  }
  if (definitive) return { matchedRuleIds, coverageMode, intent, definitive };
  if (MULTI_SOURCE_KEYWORDS.some(keyword => question.includes(keyword))) {
    matchedRuleIds.push(RULE_MULTI_SOURCE);
    coverageMode = "broad";
    intent = "cross_source_synthesis";
  } else if (FACT_LOOKUP_KEYWORDS.some(keyword => question.includes(keyword))) {
    matchedRuleIds.push(RULE_FACT_LOOKUP);
    coverageMode = "high_recall";
    intent = "fact_lookup";
  }
  return { matchedRuleIds, coverageMode: matchedRuleIds.length > 0 ? coverageMode : null, intent, definitive };
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

/**
 * LLM 判 exhaustive 的置信度门槛（§三十二"置信度较低时进入 broad"）：
 * 低于该值的 exhaustive 判定保守降为 broad（完整性义务随档位一并撤销，
 * 维持 requiresCompleteness ⟺ exhaustive 不变量）。
 */
export const KNOWLEDGE_COVERAGE_EXHAUSTIVE_MIN_CONFIDENCE = 0.6;

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
function rulesOnlyPlan(input: {
  rules: CoverageRuleMatch;
  info: KnowledgeCoverageTurnScopeInfo | null | undefined;
  degradeReason?: CoverageDegradeReason;
}): KnowledgeCoveragePlan {
  const metadataScope = coverageScopeLevelFromMetadata(input.info);
  if (input.rules.coverageMode === "broad") {
    return {
      intent: input.rules.intent ?? "cross_source_synthesis",
      coverageMode: "broad",
      requiresCompleteness: false,
      scopeLevel: metadataScope === "multi_notebook" ? "multi_notebook" : "multi_source",
      confidence: 0.8,
      matchedRuleIds: [...input.rules.matchedRuleIds],
      classifierUsed: "rules",
    };
  }
  if (input.rules.coverageMode === "high_recall") {
    return {
      intent: input.rules.intent ?? "fact_lookup",
      coverageMode: "high_recall",
      requiresCompleteness: false,
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
    requiresCompleteness: false,
    scopeLevel: metadataScope,
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
  const metadataScope = coverageScopeLevelFromMetadata(input.info);
  const scopeLevel = input.rules.coverageMode === "broad"
    ? (metadataScope === "multi_notebook" ? "multi_notebook" : "multi_source")
    : input.rules.coverageMode === "high_recall"
      ? "local"
      : metadataScope;
  return {
    intent: "fact_lookup",
    coverageMode: "high_recall",
    requiresCompleteness: false,
    scopeLevel,
    confidence: 0.3,
    matchedRuleIds: [...input.rules.matchedRuleIds],
    classifierUsed: "rules",
    degradeReason: input.reason,
  };
}

/**
 * 语义层判定归并：执行 requiresCompleteness ⟺ exhaustive 不变量与低置信
 * exhaustive → broad 的保守降档；global_negative 意图蕴含完整性义务。
 */
function mergeClassification(input: {
  rules: CoverageRuleMatch;
  info: KnowledgeCoverageTurnScopeInfo | null | undefined;
  classification: CoverageClassification;
}): KnowledgeCoveragePlan {
  const { classification } = input;
  const requiresCompleteness = classification.requiresCompleteness || classification.intent === "global_negative";
  let coverageMode: KnowledgeCoverageMode;
  if (requiresCompleteness) {
    coverageMode = "exhaustive";
  } else if (classification.coverageMode === "exhaustive") {
    coverageMode = classification.confidence >= KNOWLEDGE_COVERAGE_EXHAUSTIVE_MIN_CONFIDENCE
      ? "exhaustive"
      : "broad";
  } else {
    coverageMode = classification.coverageMode;
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
    requiresCompleteness: coverageMode === "exhaustive",
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
 * - exhaustive/global-negative 规则命中 → 直接定档（§四十一），不调 LLM；
 * - 其余档位：classifyModel 可用 → 一次严格 JSON 分类（纠错重试一次），
 *   连续无效/调用失败 → 降级 high_recall 并留 degradeReason；
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
  if (rules.definitive) {
    return {
      intent: rules.intent ?? "whole_scope_analysis",
      coverageMode: "exhaustive",
      requiresCompleteness: true,
      scopeLevel: coverageScopeLevelFromMetadata(input.turnScopeInfo),
      confidence: 0.95,
      matchedRuleIds: [...rules.matchedRuleIds],
      classifierUsed: "rules",
    };
  }
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
