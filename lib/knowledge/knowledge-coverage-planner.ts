/** 旧覆盖计划的历史读取类型与校验；生产调查不再生成旧计划或调用旧分类器。 */
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
  /** 遗留列：两档化后新行恒 false；存量 exhaustive 行为 true（仅读取兼容）。 */
  requiresCompleteness: boolean;
  scopeLevel: KnowledgeCoverageScopeLevel;
  subQueries: string[];
  confidence: number;
  matchedRuleIds: string[];
  classifierUsed: KnowledgeCoverageClassifierUsed;
  /** 持久化列可空归一（plan 里的可选字段在行里是 null）。 */
  degradeReason: string | null;
}


const COVERAGE_INTENTS = new Set<KnowledgeCoverageIntent>([
  "fact_lookup",
  "cross_source_synthesis",
  "whole_scope_analysis",
  "global_negative",
  "open_summary",
]);

/** 校验集含 'exhaustive'：LLM 输出与存量持久化行的旧值读取兼容（执行侧映射 broad）。 */
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
