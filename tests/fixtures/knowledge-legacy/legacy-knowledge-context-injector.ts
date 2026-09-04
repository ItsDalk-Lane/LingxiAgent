import { KNOWLEDGE_EVIDENCE_BUDGET } from "./legacy-query-service.ts";
/** 已退役行为的历史回归夹具，不进入生产导出或运行闭包。 */
/** 仅供历史测试与显式兼容入口使用；新的详细请求不得进入旧拆解、扩展、补查及滚动编排。 */
/**
 * knowledge-context-injector —— 主界面笔记本引用的拆解 + 检索 + 注入块生成
 * （Phase 8：HIGH_RECALL / BROAD 两档执行侧；2026-08-31：EXHAUSTIVE 档与
 * 蒸馏压缩路径移除，超预算改走 knowledge-rollup 主模型滚动多轮注入）。
 *
 * 纯函数化可测：模型调用与检索门面全部依赖注入，本模块不做 IO。
 * desktop-session-submit 在用户可见投影确定之后把返回的注入块拼进发给模型的
 * prompt；注入块是系统侧指引文本（英文、不走 locale），绝不进入用户投影。
 *
 * 覆盖执行（Phase 8 消费 plan，§三十三~§四十一）：
 * - candidate budgets（§二十六）：generation → fusion → rerank → evidence →
 *   injection 逐级截断，topK（含 NULL→1000）不再是覆盖机制；
 * - high_recall（§三十三/§三十四）：直检 + 拆解并行（Recall Safety Net）+
 *   受控扩展（≤3，§三十五）+ 邻接扩展（§三十六，contextOnly）；
 * - broad（§三十七~§三十九）：Source Coverage Floor / Section Coverage 的
 *   constrained 二次探测，无果如实记 no relevant evidence，绝不硬塞；
 * - 超预算（2026-08-31 起）：证据总量超出注入预算 → runKnowledgeRollup 把
 *   证据拆成 N 份滚动喂给会话主模型做中间消化（紧凑笔记，逐部分标注），
 *   最后一部分与全部中间笔记进入最终注入块；循环内模型可用
 *   ```need-more-evidence``` fenced 块自主发起补充检索（走既有 retrieve 门面，
 *   轮数/查询数有硬上限）。滚动不可用/失败 → 降级预算截断 + 分片清单并留痕
 *   （禁静默）。存量持久化 plan 的旧值 'exhaustive' 一律按 broad 执行。
 * - 自动升级（§四十一 执行侧）：footprint 不足 → 补 broad 轮（stats.upgradedTo）；
 * - footprint（§四十）：stats 携带触达率计数——chunkRecallFootprint 只是触达率，
 *   绝不是 actual recall。
 *
 * 降级规则（禁静默降级红线）：拆解或检索的任何失败都在注入块内显式留痕
 * （[question decomposition unavailable: ...] / [knowledge retrieval unavailable: ...]），
 * 不悄悄退回无注入的普通聊天。
 */
import { estimateTextTokens } from "../../../lib/llm/estimate-text-tokens.ts";
import type { KnowledgeReferenceMode, KnowledgeRetrievalStats } from "../../../shared/knowledge-refs.ts";
import { KnowledgeError } from "../../../lib/knowledge/errors.ts";
import type { KnowledgeCoveragePlan } from "../../../lib/knowledge/knowledge-coverage-planner.ts";
import { runKnowledgeRollup, type KnowledgeRollupEntry } from "./knowledge-rollup.ts";
import type { NotebookRetrievalChunk, NotebookRetrievalSource, RetrieveForNotebooksResult } from "../../../lib/knowledge/knowledge-query-service.ts";
import { knowledgeSectionKeyOf } from "../../../lib/knowledge/knowledge-query-service.ts";
import { KNOWLEDGE_CANDIDATE_GENERATION_BUDGET } from "./legacy-query-service.ts";
import type { QuestionIntent, DecomposeSpecialistKind, DecomposeModel, QuestionComplexity, QuestionExpansion, QueryExpansionModel, QueryExpansionResult, QuestionDecomposition, DecomposeDegradeReason, DecomposeResult, KnowledgeInjectorDeps, KnowledgeEvidenceEntry, KnowledgeCoverageFootprint, BroadProbeOutcome, KnowledgeInjectionEvidence, KnowledgeExecutionStats } from "./legacy-context-renderer.ts";
import { KNOWLEDGE_INJECTION_FALLBACK_BUDGET_TOKENS, mergeSources, resolveFusionPoolBudget, fuseQueryFamilies, groupFamiliesById, rrfFuseRankings, chunkHeader, quoteText, markScannedEvidence, renderKnowledgeContextBlock } from "./legacy-context-renderer.ts";
export type { QuestionIntent, DecomposeSpecialistKind, QuestionComplexity, QuestionDecomposition, DecomposeDegradeReason, DecomposeResult, DecomposeModel, QueryExpansionModel, QueryExpansionDegradeReason, QueryExpansionResult, KnowledgeInjectorDeps, KnowledgeEvidenceEntry, KnowledgeEvidenceIdentityEntry, KnowledgeInjectionEvidence, KnowledgeCoverageFootprint, KnowledgeExecutionStats } from "./legacy-context-renderer.ts";
export { KNOWLEDGE_INJECTION_FALLBACK_BUDGET_TOKENS, KNOWLEDGE_FUSION_POOL_UTILIZATION, KNOWLEDGE_FUSION_POOL_MAX, resolveFusionPoolBudget, resolveKnowledgeInjectionBudgetTokens, fuseSubQueryResults, fuseQueryFamilies, groupFamiliesById, knowledgeModeGuidance, knowledgeRollupGuidance, renderKnowledgeContextBlock, assembleKnowledgeEvidenceManifestEntries, EvidencePacker } from "./legacy-context-renderer.ts";


/**
 * 证据锚点数随注入预算伸缩（2026-08-30）：Phase 8 的固定 40 锚点在大会话模型
 * 下只占预算一成（实测 512k 窗口 → ~50 万 token 预算 vs 40 块 ≈ 5 万 token），
 * 余量闲置。公式：按融合候选的平均 token 估算为粒度，锚点最多吃掉预算的
 * KNOWLEDGE_EVIDENCE_BUDGET_UTILIZATION（另一半留给邻接扩展与块头开销）；
 * 下限 = KNOWLEDGE_EVIDENCE_BUDGET（40，小预算模型既有行为兜底，装填循环
 * 仍按预算硬裁不会超），上限 = KNOWLEDGE_EVIDENCE_BUDGET_MAX（240，防碎片
 * 块语料把 prompt 切成碎屑）。fused 为空/预算非法 → 下限。
 */
export const KNOWLEDGE_EVIDENCE_BUDGET_UTILIZATION = 0.5;
export const KNOWLEDGE_EVIDENCE_BUDGET_MAX = 240;

/**
 * 候选总预算（§二十一，2026-08-30 拆解优化）：查询数不再隐式放大下游成本
 * （每查询 × 每笔记本各一次嵌入+rerank）。子查询/扩展查询的每查询 topK =
 * 总预算对（非等值子查询 + 直检）的分摊，下限 24（保检索意义）、上限 =
 * KNOWLEDGE_CANDIDATE_GENERATION_BUDGET（60，既有单查询水位）。直检在 t0
 * 已启动不追溯，天然占满 60。
 */
export const KNOWLEDGE_TOTAL_CANDIDATE_BUDGET = 240;
export const KNOWLEDGE_TOTAL_CANDIDATE_BUDGET_MIN_PER_QUERY = 24;

/**
 * 快速档注入封顶（2026-08-31 两档化）：快速档以最快速度回答为主——锚点硬
 * 封顶 KNOWLEDGE_FAST_MAX_EVIDENCE_ENTRIES 条（高命中头部证据，不随预算
 * 伸缩）、渲染预算收紧 KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS（不随模型窗口
 * 放大，不触发滚动消化）。详细档维持既有预算链（预算倒推 + 超预算滚动）。
 */
export const KNOWLEDGE_FAST_MAX_EVIDENCE_ENTRIES = 12;
export const KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS = 8192;
/**
 * 快速档 rerank 期限（默认 15s 的收紧版）：门控放行重排（结果模糊）时也
 * 最多等 5s——超时降级 RRF 名次（既有降级路径），快速档的等待有界。
 */
export const KNOWLEDGE_FAST_RERANK_DEADLINE_MS = 5000;

/** 伸缩后的证据锚点上限：确定性纯函数（同输入同输出，便于测试与留痕）。 */
export function resolveEvidenceAnchorBudget(input: {
  budgetTokens: number;
  fused: ReadonlyArray<{ text: string }>;
}): number {
  if (input.fused.length === 0) return KNOWLEDGE_EVIDENCE_BUDGET;
  if (!Number.isFinite(input.budgetTokens) || input.budgetTokens <= 0) {
    return KNOWLEDGE_EVIDENCE_BUDGET;
  }
  const totalTokens = input.fused.reduce((sum, chunk) => sum + estimateTextTokens(chunk.text), 0);
  const avgTokens = Math.max(1, totalTokens / input.fused.length);
  const scaled = Math.floor((input.budgetTokens * KNOWLEDGE_EVIDENCE_BUDGET_UTILIZATION) / avgTokens);
  return Math.max(
    KNOWLEDGE_EVIDENCE_BUDGET,
    Math.min(KNOWLEDGE_EVIDENCE_BUDGET_MAX, scaled),
  );
}

const DECOMPOSE_SUBQUERY_MAX = 4;
const DECOMPOSE_SUBQUERY_MAX_CHARS = 500;
const DECOMPOSE_OUTPUT_MAX_CHARS = 10_000;

/**
 * 受控查询扩展硬上限（§三十五）：扩展查询（paraphrase/synonym/entity 归一）至多
 * 3 条，与拆解子查询共用总查询预算（直检 1 + 子查询 ≤4 + 扩展 ≤3）。禁止无限
 * LLM 扩展——query × notebook × FTS × vector 的指数膨胀在这里被切断。
 */
export const KNOWLEDGE_QUERY_EXPANSION_MAX = 3;
const EXPANSION_OUTPUT_MAX_CHARS = 10_000;
const EXPANSION_QUERY_MAX_CHARS = 500;

/**
 * 邻接扩展窗口（§三十六）：检索锚点向前后各扩展的 ordinal 数（同 variant 内）。
 * 窗口可由调用方按语料粒度覆写；0 = 关闭邻接扩展。
 */
export const KNOWLEDGE_NEIGHBOR_EXPANSION_WINDOW = 1;

/**
 * §四十一 执行侧自动升级阈值：high_recall 执行后 sourceCoverageFootprint 低于
 * 该值且多源 scope（selectedSourceCount ≥ KNOWLEDGE_AUTO_UPGRADE_MIN_SOURCES）时
 * 自动补一轮 broad 流程（复用已检索结果，只补缺失探测）。
 */
export const KNOWLEDGE_AUTO_UPGRADE_SOURCE_FOOTPRINT_MIN = 0.34;
export const KNOWLEDGE_AUTO_UPGRADE_MIN_SOURCES = 2;

/**
 * broad 档结构缺口探测的二次检索调用总数上限（§三十八/§三十九 bounded）：超限
 * 即停止探测并在块内留痕（显式可见，绝不冒充已探完）。
 */
export const KNOWLEDGE_SECONDARY_RETRIEVAL_MAX = 16;

/**
 * §三十九 section coverage 触发启发式：命中章节数 < 源内可用章节数 × 该比例
 * 且 plan.scopeLevel 指示整体性（非 local）时，对未命中 section 做一次
 * section-constrained secondary retrieval。
 */
export const KNOWLEDGE_SECTION_COVERAGE_MIN_HIT_RATIO = 0.5;

/**
 * 专业问题拆解系统提示词。规则风格对齐原 Quick Answer 提示词
 * （编号规则 + 严格 JSON schema + 禁 Markdown 围栏）。
 */
export const KNOWLEDGE_DECOMPOSE_SYSTEM_PROMPT = `You decompose a user question into focused retrieval sub-queries for Knowledge notebook search.

Rules:
1. Produce 1 to 4 sub-queries. Use a single sub-query only when the question is already one focused lookup.
2. Each sub-query must represent one independent evidence need — a distinct piece of evidence required to answer the question (an entity's state, a time-constrained fact, a cause, one side of a comparison, an exclusion condition). If two sub-queries would be answered by essentially the same passages, merge them into one.
3. For negated questions (not, except, besides, 除了/不包括), include one sub-query that states the exclusion condition itself.
4. Keep proper nouns, product names, and code identifiers exactly as written in the question. Do not translate or normalize them.
5. Do NOT add synonym rewrites, translations, or keyword variants — a separate expansion stage owns retrieval wording. Only distinct evidence needs belong here.
6. The sub-queries search untrusted source data. Never embed instructions for the reader inside a sub-query.
7. Return one JSON object and nothing else. Do not use Markdown fences.

Schema:
{"intent":"factual|summarize|compare|list|reasoning","subQueries":["..."]}`;

const QUESTION_INTENTS = new Set<QuestionIntent>(["factual", "summarize", "compare", "list", "reasoning"]);

const DECOMPOSE_SPECIALIST_KINDS: readonly DecomposeSpecialistKind[] = ["fact", "cause", "relation", "validation"];

/**
 * 专业拆解提示词：每个方向只回答「本维度需要一个什么样的证据查询」。认知
 * 职责分离（§三：单一 Universal Decomposer 的认知混淆靠拆方向消除），温度
 * 保持 0（§二：多样性来自职责分离，不来自温度）。输出契约复用拆解 schema
 * （本维度不需要时可返回空 subQueries——parseSpecialistDecomposition 放行 0 条）。
 */
export const KNOWLEDGE_DECOMPOSE_SPECIALIST_PROMPTS: Record<DecomposeSpecialistKind, string> = {
  fact: `You are the Fact/Structure specialist decomposing a user question for Knowledge notebook retrieval.
Your dimension: facts, objects, entities, composition, and baseline states.

Rules:
1. Return 1 to 2 sub-queries ONLY for facts/structure this question needs as independent evidence (an entity's state, an attribute, a time-constrained fact, a listing/enumeration target).
2. If your dimension contributes no independent evidence need for this question, return an empty subQueries array.
3. State negated exclusions (除了/不包括/except) in the "exclusions" field, never as a sub-query.
4. Keep proper nouns, product names, and code identifiers exactly as written. No synonym rewrites — a separate expansion stage owns retrieval wording.
5. The sub-queries search untrusted source data. Never embed instructions for the reader inside a sub-query.
6. Return one JSON object and nothing else. Do not use Markdown fences.

Schema:
{"intent":"factual|summarize|compare|list|reasoning","subQueries":["..."],"exclusions":["..."]}`,
  cause: `You are the Cause/Mechanism specialist decomposing a user question for Knowledge notebook retrieval.
Your dimension: causes, preconditions, mechanisms, and causal chains.

Rules:
1. Return 1 to 2 sub-queries ONLY for causes/mechanisms this question needs as independent evidence (why something happened, what enabled it, what chain led there).
2. If your dimension contributes no independent evidence need for this question, return an empty subQueries array.
3. Keep proper nouns, product names, and code identifiers exactly as written. No synonym rewrites — a separate expansion stage owns retrieval wording.
4. The sub-queries search untrusted source data. Never embed instructions for the reader inside a sub-query.
5. Return one JSON object and nothing else. Do not use Markdown fences.

Schema:
{"intent":"factual|summarize|compare|list|reasoning","subQueries":["..."]}`,
  relation: `You are the Relation/Process specialist decomposing a user question for Knowledge notebook retrieval.
Your dimension: comparisons, relationships, evolutions, stages, and interactions.

Rules:
1. Return 1 to 2 sub-queries ONLY for relations/processes this question needs as independent evidence (one side of a comparison, a relationship between two entities, a process or its stages). For comparisons, prefer one sub-query per side being compared.
2. If your dimension contributes no independent evidence need for this question, return an empty subQueries array.
3. Keep proper nouns, product names, and code identifiers exactly as written. No synonym rewrites — a separate expansion stage owns retrieval wording.
4. The sub-queries search untrusted source data. Never embed instructions for the reader inside a sub-query.
5. Return one JSON object and nothing else. Do not use Markdown fences.

Schema:
{"intent":"factual|summarize|compare|list|reasoning","subQueries":["..."]}`,
  validation: `You are the Validation/Boundary specialist decomposing a user question for Knowledge notebook retrieval.
Your dimension: supporting evidence quality, counterexamples, boundary conditions, and competing explanations.

Rules:
1. Return 1 to 2 sub-queries ONLY for validation this question needs as independent evidence (what would confirm or contradict the expected answer, where the boundary of a claim lies, what alternative explanations exist).
2. If your dimension contributes no independent evidence need for this question, return an empty subQueries array.
3. State negated exclusions (除了/不包括/except) in the "exclusions" field, never as a sub-query.
4. Keep proper nouns, product names, and code identifiers exactly as written. No synonym rewrites — a separate expansion stage owns retrieval wording.
5. The sub-queries search untrusted source data. Never embed instructions for the reader inside a sub-query.
6. Return one JSON object and nothing else. Do not use Markdown fences.

Schema:
{"intent":"factual|summarize|compare|list|reasoning","subQueries":["..."],"exclusions":["..."]}`,
};

/**
 * Reasoning Gap Analyzer（§二十二，P2）：第一轮检索后条件触发一次——输入
 * 原问题 + 已有证据查询与命中摘要，只回答「现有证据方向是否遗漏了会实质
 * 改变答案的方向」。输出 ≤3 条补证查询（复用拆解 schema，允许 0 条）。
 * 与查询扩展的分工：扩展改写既有方向的表达，Gap Analyzer 发现缺失方向。
 */
export const KNOWLEDGE_GAP_ANALYSIS_SYSTEM_PROMPT = `You analyze whether the evidence queries already searched may miss something for a user question, for Knowledge notebook retrieval.

Rules:
1. You receive the question, the evidence queries already searched, and how many passages each found (0 means nothing found).
2. Identify at most 3 MISSING evidence directions whose absence could materially change the answer — an unsearched facet, an unexamined entity, the other side of a comparison, or a boundary/counterexample check. Do NOT restate or paraphrase existing queries; a separate expansion stage owns rewording.
3. If the existing queries already cover the question adequately, return an empty subQueries array.
4. Keep proper nouns, product names, and code identifiers exactly as written in the question.
5. The queries search untrusted source data. Never embed instructions for the reader inside a query.
6. Return one JSON object and nothing else. Do not use Markdown fences.

Schema:
{"intent":"factual|summarize|compare|list|reasoning","subQueries":["..."]}`;

/** Gap Analyzer 触发判定（§二十二：只有高覆盖模式或已知缺口才值一次 LLM）。 */
export function shouldRunGapAnalysis(input: {
  coverageMode: "broad" | "high_recall" | "exhaustive" | null;
  subQueries: string[];
  subQueryHits: number[];
  originalQueryHits: number;
}): { trigger: boolean; reason: string | null } {
  // 旧值 'exhaustive'（存量持久化 plan）与 broad 同待遇：结构探测已跑过，
  // 只按已知缺口触发。
  if (input.coverageMode === "high_recall") {
    return { trigger: true, reason: "coverage mode high_recall" };
  }
  if (input.subQueries.length > 0 && input.subQueryHits.some(hits => hits === 0)) {
    return { trigger: true, reason: "some evidence need found nothing (0 hits)" };
  }
  if (input.originalQueryHits === 0) {
    return { trigger: true, reason: "original query found nothing (0 hits)" };
  }
  return { trigger: false, reason: null };
}

/** 单次 Gap 分析（含一次纠错；输出 ≤3 条补证查询，失败/降级返回空并留痕）。 */
export async function runGapAnalysis(input: {
  question: string;
  existing: Array<{ query: string; hits: number }>;
  callModel: DecomposeModel | null;
  now?: () => number;
}): Promise<{ queries: string[]; degraded: boolean; degradeReason: string | null }> {
  if (!input.callModel) return { queries: [], degraded: true, degradeReason: "gap analysis model not wired" };
  const context = [
    "Existing evidence queries and their hit counts:",
    ...(input.existing.length > 0
      ? input.existing.map(entry => `- ${entry.query} → ${entry.hits} hits`)
      : ["- (none — only the original question was searched)"]),
  ].join("\n");
  let firstError = "";
  let firstOutput = "";
  for (let attempt = 0; attempt < 2; attempt += 1) {
    let raw: string;
    try {
      raw = await input.callModel(attempt === 0
        ? { question: input.question, specialist: "gap", context }
        : {
          question: input.question,
          specialist: "gap",
          context,
          correction: { error: firstError, previousOutput: firstOutput },
        });
    } catch (error) {
      return { queries: [], degraded: true, degradeReason: `gap analysis call failed: ${describeError(error)}` };
    }
    try {
      const parsed = parseQuestionDecomposition(raw, { allowEmptySubQueries: true });
      // 与既有查询（原问题 + 子查询 + 扩展）等值的补证直接丢弃；总量 ≤3。
      const seen = new Set(input.existing.map(entry => entry.query.trim()));
      const queries: string[] = [];
      for (const subQuery of parsed.subQueries) {
        if (seen.has(subQuery)) continue;
        seen.add(subQuery);
        queries.push(subQuery);
      }
      return { queries: queries.slice(0, 3), degraded: false, degradeReason: null };
    } catch (error) {
      if (attempt === 0) {
        firstError = describeError(error);
        firstOutput = raw.slice(0, 2000);
        continue;
      }
      return { queries: [], degraded: true, degradeReason: `gap analysis output invalid: ${firstError}` };
    }
  }
  return { queries: [], degraded: true, degradeReason: "gap analysis output invalid" };
}

// 注意：词标 pattern 一律不带 g 标志——/g 的 .test() 有 lastIndex 状态，
// 同一 pattern 对象跨调用会漂移结果（评估必须纯函数）。
const COMPLEXITY_MARKER_DIMENSIONS: ReadonlyArray<{ pattern: RegExp; dimension: DecomposeSpecialistKind }> = [
  { pattern: /为什么|为何|缘故|原因|导致|机制|怎么会|因果|why\b|cause/i, dimension: "cause" },
  { pattern: /比较|区别|相比|差异|对比|哪个更|哪一个更|vs\.?|和.{1,12}(一样|相同|不同)|compare|difference/i, dimension: "relation" },
  { pattern: /关系|演变|过程|阶段|流程|之间|相互作用|先后|relationship|process|stages/i, dimension: "relation" },
  { pattern: /列出|全部|所有|分别|哪些|各有哪些|各自|list all|every\b/i, dimension: "fact" },
  { pattern: /除了|不包括|排除|不含|except|besides|excluding/i, dimension: "validation" },
];
const COMPLEXITY_LOOKUP_PATTERN = /谁|什么|何时|哪年|哪一年|多少|几岁|在哪里|是谁|what\b|who\b|when\b|how many|how much|where\b/i;
const COMPLEXITY_CONNECTIVE_PATTERN = /、|和|与|及|并且|同时|以及|还是|还是说|以及是否|\band\b|\bor\b/gi;

/**
 * 廉价复杂度闸（§五：不加 LLM Router——规则 + 词标判定）。simple 判定刻意
 * 保守：必须同时满足「查表式问句 + 无任何维度词标 + 短文本 + 无并列连接」，
 * 疑难归 focused（宁可多一次拆解，不误跳过）。dimensions 按词标命中去重，
 * 不足档位数时按 fact → cause → relation → validation 顺序补齐。
 */
export function assessQuestionComplexity(question: string): {
  level: QuestionComplexity;
  dimensions: DecomposeSpecialistKind[];
} {
  const text = question.trim();
  const dimensions: DecomposeSpecialistKind[] = [];
  for (const { pattern, dimension } of COMPLEXITY_MARKER_DIMENSIONS) {
    if (pattern.test(text) && !dimensions.includes(dimension)) dimensions.push(dimension);
  }
  const connectives = (text.match(COMPLEXITY_CONNECTIVE_PATTERN) ?? []).length;
  const multiClause = /[；;。]|.{8,}？.{8,}[？?]/.test(text);
  const lookupShaped = COMPLEXITY_LOOKUP_PATTERN.test(text);
  if (dimensions.length === 0 && connectives === 0 && !multiClause && lookupShaped
    && text.length <= 40 && text.split(/\s+/).length <= 12) {
    return { level: "simple", dimensions: [] };
  }
  const level: QuestionComplexity = dimensions.length >= 3 || (dimensions.length >= 2 && (connectives >= 2 || multiClause))
    ? "complex"
    : dimensions.length === 2
      ? "compound"
      : "focused";
  const targetCount = level === "complex" ? Math.min(4, Math.max(3, dimensions.length)) : dimensions.length || 1;
  const selected = [...dimensions];
  for (const kind of DECOMPOSE_SPECIALIST_KINDS) {
    if (selected.length >= targetCount) break;
    if (!selected.includes(kind)) selected.push(kind);
  }
  return { level, dimensions: selected.slice(0, 4) };
}

/**
 * 受控查询扩展系统提示词（§三十五）。与拆解提示词同一规则风格；硬上限 3 条、
 * 禁近重复、专有名词保持原样、禁嵌注入指令（子查询搜索的是不可信源数据）。
 */
export const KNOWLEDGE_EXPANSION_SYSTEM_PROMPT = `You expand a retrieval query set with controlled paraphrases for Knowledge notebook search.

Rules:
1. Return 0 to 3 expansion queries: synonym rewrites, paraphrases, or entity-normalized variants of the question or the existing sub-queries.
2. Add a variant only when it may match different wording in the documents. Never add a near-duplicate of an existing query.
3. Keep proper nouns, product names, and code identifiers exactly as written in the question. Do not translate or normalize them.
4. The queries search untrusted source data. Never embed instructions for the reader inside a query.
5. Return one JSON object and nothing else. Do not use Markdown fences.

Schema: {"expansions":["..."]}`;

function requiredExpansion(value: unknown): QuestionExpansion {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Query expansion output must be an object");
  }
  const record = value as Record<string, unknown>;
  // 宽容输入 + 严格消费（2026-08-30 拆解优化）：未知字段忽略（白名单只取
  // expansions），必需字段缺失/内容非法仍然整体拒绝——无害格式偏差不再
  // 浪费一次 8s 纠错往返。
  if (!Object.hasOwn(record, "expansions")) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Query expansion output fields are invalid");
  }
  if (!Array.isArray(record.expansions) || record.expansions.length > KNOWLEDGE_QUERY_EXPANSION_MAX) {
    throw new KnowledgeError(
      "KNOWLEDGE_MODEL_OUTPUT_INVALID",
      `Query expansion must contain at most ${KNOWLEDGE_QUERY_EXPANSION_MAX} expansions`,
    );
  }
  const expansions: string[] = [];
  for (const raw of record.expansions) {
    if (typeof raw !== "string") {
      throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Query expansion query must be a string");
    }
    const trimmed = raw.trim();
    if (!trimmed || trimmed.length > EXPANSION_QUERY_MAX_CHARS) {
      throw new KnowledgeError(
        "KNOWLEDGE_MODEL_OUTPUT_INVALID",
        `Query expansion query must be non-empty and at most ${EXPANSION_QUERY_MAX_CHARS} characters`,
      );
    }
    expansions.push(trimmed);
  }
  return { expansions };
}

/**
 * 解析并严格校验扩展输出：纯 JSON、唯一字段 expansions、0-3 条非空且不超长。
 * 与既有查询（原问题 + 子查询）等值的条目直接丢弃（受控去重，不算失败）。
 */
export function parseQueryExpansion(raw: string, existingQueries: string[]): string[] {
  if (typeof raw !== "string" || !raw.trim() || raw.length > EXPANSION_OUTPUT_MAX_CHARS) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Query expansion model output is empty or too large");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(stripModelOutputFences(raw));
  } catch {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Query expansion model output is not valid JSON");
  }
  const { expansions } = requiredExpansion(parsed);
  const seen = new Set(existingQueries.map(query => query.trim()));
  const deduped: string[] = [];
  for (const expansion of expansions) {
    if (seen.has(expansion)) continue;
    seen.add(expansion);
    deduped.push(expansion);
  }
  return deduped;
}

/**
 * 受控查询扩展（§三十五）：拆解成功后追加 ≤3 条 paraphrase/synonym 扩展。输出
 * 非法 → 纠错重试一次；连续无效或调用失败 → 不扩展并留痕（禁静默降级）。
 * 硬上限由 parse 层强制；任何失败都不阻断主检索链。
 */
export async function expandQueries(input: {
  question: string;
  existingQueries: string[];
  callModel: QueryExpansionModel | null;
}): Promise<QueryExpansionResult> {
  if (!input.callModel) {
    return {
      expansions: [],
      attempted: false,
      degraded: true,
      degradeReason: "knowledge model slot not configured",
      degradeDetail: null,
    };
  }
  let firstError = "";
  let firstOutput = "";
  for (let attempt = 0; attempt < 2; attempt += 1) {
    let raw: string;
    try {
      raw = await input.callModel(attempt === 0
        ? { question: input.question, existingQueries: input.existingQueries }
        : {
          question: input.question,
          existingQueries: input.existingQueries,
          correction: { error: firstError, previousOutput: firstOutput },
        });
    } catch (error) {
      return {
        expansions: [],
        attempted: true,
        degraded: true,
        degradeReason: "model call failed",
        degradeDetail: describeError(error),
      };
    }
    try {
      const expansions = parseQueryExpansion(raw, [input.question, ...input.existingQueries]);
      return { expansions, attempted: true, degraded: false, degradeReason: null, degradeDetail: null };
    } catch (error) {
      if (attempt === 0) {
        firstError = describeError(error);
        firstOutput = raw.slice(0, 2000);
        continue;
      }
      return {
        expansions: [],
        attempted: true,
        degraded: true,
        degradeReason: "model output invalid after one correction retry",
        degradeDetail: firstError,
      };
    }
  }
  // 循环必然 return；此处仅为类型完备。
  return {
    expansions: [],
    attempted: true,
    degraded: true,
    degradeReason: "model output invalid after one correction retry",
    degradeDetail: null,
  };
}

function requiredDecomposition(
  value: unknown,
  options?: { allowEmptySubQueries?: boolean },
): QuestionDecomposition {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Decomposition output must be an object");
  }
  const record = value as Record<string, unknown>;
  // 宽容输入 + 严格消费（2026-08-30 拆解优化）：未知字段（reason/foo/…）忽略，
  // 只白名单消费 intent + subQueries；必需字段缺失或内容非法仍整体拒绝。
  // 提示词仍严格要求恰好两字段——解析侧不再因无害偏差丢弃整个结果。
  if (!Object.hasOwn(record, "intent") || !Object.hasOwn(record, "subQueries")) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Decomposition output fields are invalid");
  }
  if (typeof record.intent !== "string" || !QUESTION_INTENTS.has(record.intent as QuestionIntent)) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Decomposition intent is invalid");
  }
  const allowEmpty = options?.allowEmptySubQueries ?? false;
  if (!Array.isArray(record.subQueries) || record.subQueries.length > DECOMPOSE_SUBQUERY_MAX
    || (!allowEmpty && record.subQueries.length < 1)) {
    throw new KnowledgeError(
      "KNOWLEDGE_MODEL_OUTPUT_INVALID",
      `Decomposition must contain ${allowEmpty ? "0 to" : "1 to"} ${DECOMPOSE_SUBQUERY_MAX} sub-queries`,
    );
  }
  const seen = new Set<string>();
  const subQueries: string[] = [];
  for (const raw of record.subQueries) {
    if (typeof raw !== "string") {
      throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Decomposition sub-query must be a string");
    }
    const trimmed = raw.trim();
    if (!trimmed || trimmed.length > DECOMPOSE_SUBQUERY_MAX_CHARS) {
      throw new KnowledgeError(
        "KNOWLEDGE_MODEL_OUTPUT_INVALID",
        `Decomposition sub-query must be non-empty and at most ${DECOMPOSE_SUBQUERY_MAX_CHARS} characters`,
      );
    }
    if (seen.has(trimmed)) continue;
    seen.add(trimmed);
    subQueries.push(trimmed);
  }
  if (subQueries.length === 0 && !allowEmpty) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Decomposition produced no usable sub-queries");
  }
  // 否定排除条件（§九，P2）：可选白名单字段——词法约束，≤4 条、每条 ≤100
  // 字符、trimmed 去重；非法形状整体拒绝（宁可少一条约束也不静默吞错）。
  const exclusions: string[] = [];
  if (record.exclusions != null) {
    if (!Array.isArray(record.exclusions) || record.exclusions.length > 4) {
      throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Decomposition exclusions must be an array of at most 4 strings");
    }
    const seenExclusions = new Set<string>();
    for (const raw of record.exclusions) {
      if (typeof raw !== "string") {
        throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Decomposition exclusion must be a string");
      }
      const trimmed = raw.trim();
      if (!trimmed || trimmed.length > 100) {
        throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Decomposition exclusion must be non-empty and at most 100 characters");
      }
      if (seenExclusions.has(trimmed)) continue;
      seenExclusions.add(trimmed);
      exclusions.push(trimmed);
    }
  }
  return { intent: record.intent as QuestionIntent, subQueries, exclusions };
}

/**
 * 程序化格式修复（2026-08-30 拆解优化 §14）：剥离 ```json 围栏与首尾空白后再
 * JSON.parse——纯格式偏差（fence/空白）不烧 8s 纠错往返，直接程序修复；语义
 * 层非法（字段/枚举/条数）仍走纠错。仅剥离「整段被围栏包裹」的形状，不动
 * JSON 内部内容。
 */
export function stripModelOutputFences(raw: string): string {
  const trimmed = raw.trim();
  const fenced = /^```[a-zA-Z0-9_-]*\s*([\s\S]*?)\s*```$/.exec(trimmed);
  return fenced ? fenced[1].trim() : trimmed;
}

/**
 * 解析并严格校验拆解输出（requiredObject 风格）：纯 JSON、精确字段、
 * intent 枚举、子查询 1-4 条非空且不超长。任何不符抛 KNOWLEDGE_MODEL_OUTPUT_INVALID。
 */
export function parseQuestionDecomposition(
  raw: string,
  options?: { allowEmptySubQueries?: boolean },
): QuestionDecomposition {
  if (typeof raw !== "string" || !raw.trim() || raw.length > DECOMPOSE_OUTPUT_MAX_CHARS) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Decomposition model output is empty or too large");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(stripModelOutputFences(raw));
  } catch {
    throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Decomposition model output is not valid JSON");
  }
  return requiredDecomposition(parsed, options);
}

function degrade(
  question: string,
  reason: DecomposeDegradeReason,
  detail: string | null,
  telemetry: { latencyMs: number; attempts: number },
  extras?: Partial<Pick<DecomposeResult, "complexity" | "specialists" | "specialistFailures">>,
): DecomposeResult {
  return {
    subQueries: [question],
    intent: null,
    degraded: true,
    degradeReason: reason,
    degradeDetail: detail,
    exclusions: [],
    complexity: null,
    specialists: [],
    specialistFailures: [],
    ...telemetry,
    ...extras,
  };
}

function describeError(error: unknown): string {
  if (error instanceof KnowledgeError) return `${error.code}: ${error.message}`;
  if (error instanceof Error) return error.message;
  return String(error);
}

/**
 * 专业问题拆解：槽位未配置 → 直接单查询（显式标注）；输出非法 → 纠错重试一次；
 * 连续无效或调用失败 → 降级为原问题单查询并在注入块标注
 * [question decomposition unavailable: ...]（显式留痕，禁静默降级）。
 */
export async function decomposeQuestion(input: {
  question: string;
  callModel: DecomposeModel | null;
  now?: () => number;
  /** 专业方向（P2）：透传给闭包选提示词；specialist 模式允许 0 条子查询。 */
  specialist?: DecomposeSpecialistKind | null;
}): Promise<DecomposeResult> {
  const question = input.question.trim();
  const now = input.now ?? (() => Date.now());
  const startedAt = now();
  const allowEmpty = input.specialist != null;
  if (!input.callModel) {
    return degrade(question, "knowledge model slot not configured", null, { latencyMs: 0, attempts: 0 });
  }
  let firstError = "";
  let firstOutput = "";
  for (let attempt = 0; attempt < 2; attempt += 1) {
    let raw: string;
    try {
      raw = await input.callModel(attempt === 0
        ? { question, ...(input.specialist ? { specialist: input.specialist } : {}) }
        : {
          question,
          correction: { error: firstError, previousOutput: firstOutput },
          ...(input.specialist ? { specialist: input.specialist } : {}),
        });
    } catch (error) {
      return degrade(question, "model call failed", describeError(error), {
        latencyMs: now() - startedAt,
        attempts: attempt + 1,
      });
    }
    try {
      const parsed = parseQuestionDecomposition(raw, { allowEmptySubQueries: allowEmpty });
      return {
        subQueries: parsed.subQueries,
        intent: parsed.intent,
        degraded: false,
        degradeReason: null,
        degradeDetail: null,
        latencyMs: now() - startedAt,
        attempts: attempt + 1,
        exclusions: parsed.exclusions,
        complexity: null,
        specialists: input.specialist ? [input.specialist] : [],
        specialistFailures: [],
      };
    } catch (error) {
      if (attempt === 0) {
        firstError = describeError(error);
        firstOutput = raw.slice(0, 2000);
        continue;
      }
      return degrade(question, "model output invalid after one correction retry", firstError, {
        latencyMs: now() - startedAt,
        attempts: 2,
      });
    }
  }
  // 循环必然 return；此处仅为类型完备。
  return degrade(question, "model output invalid after one correction retry", null, {
    latencyMs: now() - startedAt,
    attempts: 2,
  });
}

/**
 * Adaptive Specialist Decomposition（P2，§三/§四/§五）：规则复杂度闸（零 LLM）
 * 决定 0/1/2/3-4 个专业方向，方向间并行（墙钟 ≈ 单次调用），各自只回答本维度
 * 的证据需求；按 fact → cause → relation → validation 序合并去重、总量 ≤4。
 * simple 档完全跳过拆解 LLM（直检即全部，§四 Simple→0）。任一方向成功即不
 * 降级（部分失败留痕）；全部失败/未配槽位才降级为原问题单查询。
 */
export async function decomposeQuestionAdaptive(input: {
  question: string;
  callModel: DecomposeModel | null;
  now?: () => number;
}): Promise<DecomposeResult> {
  const question = input.question.trim();
  const now = input.now ?? (() => Date.now());
  const startedAt = now();
  if (!input.callModel) {
    return degrade(question, "knowledge model slot not configured", null, { latencyMs: 0, attempts: 0 });
  }
  const { level, dimensions } = assessQuestionComplexity(question);
  if (level === "simple") {
    return {
      subQueries: [],
      intent: null,
      degraded: false,
      degradeReason: null,
      degradeDetail: null,
      latencyMs: 0,
      attempts: 0,
      exclusions: [],
      complexity: level,
      specialists: [],
      specialistFailures: [],
    };
  }
  const settled = await Promise.allSettled(
    dimensions.map(dimension => decomposeQuestion({
      question: input.question,
      callModel: input.callModel,
      ...(input.now ? { now: input.now } : {}),
      specialist: dimension,
    })),
  );
  const mergedSubQueries: string[] = [];
  const mergedExclusions: string[] = [];
  const specialistFailures: string[] = [];
  const succeededKinds: DecomposeSpecialistKind[] = [];
  let attemptsTotal = 0;
  settled.forEach((outcome, index) => {
    const dimension = dimensions[index];
    if (outcome.status === "fulfilled" && !outcome.value.degraded) {
      succeededKinds.push(dimension);
      attemptsTotal += outcome.value.attempts;
      for (const subQuery of outcome.value.subQueries) {
        if (!mergedSubQueries.includes(subQuery)) mergedSubQueries.push(subQuery);
      }
      for (const exclusion of outcome.value.exclusions) {
        if (!mergedExclusions.includes(exclusion)) mergedExclusions.push(exclusion);
      }
    } else {
      const reason = outcome.status === "rejected"
        ? describeError(outcome.reason)
        : (outcome.value.degradeReason ?? "unknown");
      specialistFailures.push(`${dimension}: ${reason}`);
    }
  });
  if (succeededKinds.length === 0) {
    // 全部方向失败：与单路拆解同语义降级（原问题单查询 + 留痕）。
    return degrade(
      question,
      "model call failed",
      specialistFailures.join("; ") || null,
      { latencyMs: now() - startedAt, attempts: attemptsTotal },
      { complexity: level, specialists: dimensions, specialistFailures },
    );
  }
  // 总量封顶 4（§四 复杂档 3~4 个证据需求）：按方向序截断。
  return {
    subQueries: mergedSubQueries.slice(0, DECOMPOSE_SUBQUERY_MAX),
    intent: null,
    degraded: false,
    degradeReason: null,
    degradeDetail: null,
    latencyMs: now() - startedAt,
    attempts: attemptsTotal,
    exclusions: mergedExclusions,
    complexity: level,
    specialists: succeededKinds,
    specialistFailures,
  };
}

/**
 * 否定排除过滤（§九，P2）：排除条件是词法约束而非检索查询——embedding 对
 * 否定的表达不可靠，「除了 X」写成查询只会召回 X 本身。融合后按词面剔除
 * 含排除词的块。过度匹配保护：剔除超过半数（且池子 >4）时疑似词面撞车
 * （排除词恰好是高频词），放弃过滤保序返回并留痕，宁可多给证据不误删。
 */
export function applyNegationExclusions(input: {
  chunks: ReadonlyArray<NotebookRetrievalChunk>;
  exclusions: ReadonlyArray<string>;
}): { kept: NotebookRetrievalChunk[]; droppedCount: number; skipped: boolean } {
  const terms = input.exclusions
    .map(term => term.trim().toLowerCase())
    .filter(term => term.length >= 2);
  if (terms.length === 0 || input.chunks.length === 0) {
    return { kept: [...input.chunks], droppedCount: 0, skipped: false };
  }
  const kept: NotebookRetrievalChunk[] = [];
  let droppedCount = 0;
  for (const chunk of input.chunks) {
    const text = chunk.text.toLowerCase();
    if (terms.some(term => text.includes(term))) {
      droppedCount += 1;
      continue;
    }
    kept.push(chunk);
  }
  if (input.chunks.length > 4 && droppedCount > input.chunks.length / 2) {
    return { kept: [...input.chunks], droppedCount: 0, skipped: true };
  }
  return { kept, droppedCount, skipped: false };
}

/**
 * §三十六 邻接扩展 + 证据组装：融合锚点按序装填，每个锚点在同 variant 内向
 * 前后各 window 个 ordinal 请求邻接块（越界/已注入的缺席）。邻接块标记
 * contextOnly：进入注入块供上下文连续性、计入 neighborExpansionCount，但不计入
 * 检索命中数/coverage footprint 分子，也不参与 rerank 输入（rerank 在检索核心
 * 内部只对锚点执行）。token 开销受注入预算约束（装填循环内逐块计费）。
 * readNeighborChunks 未接线或窗口 ≤0 → 只装填锚点（调用方未启用该能力）。
 */
export function assembleEvidenceEntries(input: {
  anchors: NotebookRetrievalChunk[];
  window: number;
  readNeighborChunks?: KnowledgeInjectorDeps["readNeighborChunks"] | null;
}): KnowledgeEvidenceEntry[] {
  const { anchors, window } = input;
  if (anchors.length === 0) return [];
  if (!input.readNeighborChunks || window <= 0) {
    return anchors.map(chunk => ({ chunk, contextOnly: false }));
  }
  const entries: KnowledgeEvidenceEntry[] = [];
  const emitted = new Set(anchors.map(chunk => chunk.id));
  anchors.forEach((anchor) => {
    entries.push({ chunk: anchor, contextOnly: false });
    const ordinals: number[] = [];
    for (let offset = -window; offset <= window; offset += 1) {
      if (offset === 0) continue;
      const ordinal = anchor.ordinal + offset;
      if (ordinal >= 0) ordinals.push(ordinal);
    }
    if (ordinals.length === 0) return;
    let neighbors: NotebookRetrievalChunk[];
    try {
      neighbors = input.readNeighborChunks({ anchor, ordinals });
    } catch {
      // 门面抛错（索引竞态损坏等）：邻接块只是上下文连续性增强，缺席不阻断
      // 锚点证据装填；检索链自身的降级/自愈留痕不受影响。
      return;
    }
    for (const neighbor of neighbors) {
      if (emitted.has(neighbor.id)) continue;
      // 双保险：门面实现必须保证同 variant（越 variant 的 ordinal 无连续性语义）。
      if (neighbor.parseArtifactId !== anchor.parseArtifactId) continue;
      if (neighbor.chunkIndexVariantId !== anchor.chunkIndexVariantId) continue;
      emitted.add(neighbor.id);
      entries.push({ chunk: neighbor, contextOnly: true });
    }
  });
  return entries;
}

export function computeCoverageFootprint(input: {
  /** 跨查询 RRF 融合后的唯一锚点序列（fusionBudget 截断后）。 */
  fused: NotebookRetrievalChunk[];
  sources: NotebookRetrievalSource[];
  candidateChunkCount: number;
}): KnowledgeCoverageFootprint {
  const selectedSourceCount = input.sources.length;
  const retrievedSourceIds = new Set(input.fused.map(chunk => chunk.sourceId));
  const retrievedSourceCount = input.sources.filter(source => retrievedSourceIds.has(source.sourceId)).length;
  const availableSectionCount = input.sources.reduce(
    (sum, source) => sum + (source.sections?.length ?? 0),
    0,
  );
  const retrievedSections = new Set<string>();
  for (const chunk of input.fused) {
    const key = knowledgeSectionKeyOf(chunk.headingPath);
    if (key != null) retrievedSections.add(`${chunk.sourceId} | ${key}`);
  }
  const totalChunks = input.sources.reduce((sum, source) => sum + (source.chunkCount || 0), 0);
  return {
    selectedSourceCount,
    retrievedSourceCount,
    availableSectionCount,
    retrievedSectionCount: retrievedSections.size,
    candidateChunkCount: input.candidateChunkCount,
    uniqueChunkCount: input.fused.length,
    sourceCoverageFootprint: selectedSourceCount > 0 ? retrievedSourceCount / selectedSourceCount : null,
    sectionCoverageFootprint: availableSectionCount > 0 ? retrievedSections.size / availableSectionCount : null,
    chunkRecallFootprint: totalChunks > 0 ? input.fused.length / totalChunks : null,
  };
}

/** 已知索引/解析不可用的源（其零命中不能记成 no relevant evidence）。 */
function degradedSourceIdsOf(results: RetrieveForNotebooksResult[]): Set<string> {
  const ids = new Set<string>();
  for (const result of results) {
    for (const entry of result.degraded ?? []) {
      if (entry.sourceId) ids.add(entry.sourceId);
    }
  }
  return ids;
}

/**
 * broad 结构探测（§三十八 Source Coverage Floor + §三十九 Section Coverage）：
 * 对零命中 ready 源用全部查询做 source-constrained 二次检索；对有命中但章节
 * 覆盖明显不足（启发式：命中章节 < 可用章节 × 阈值 且 scopeLevel 非局部）的源，
 * 对未命中 section 做 section-constrained 二次检索。全程受
 * KNOWLEDGE_SECONDARY_RETRIEVAL_MAX 约束，超限显式 capped 留痕；探测无果绝不
 * 硬塞低质 chunk，只如实记录 no relevant evidence。
 */
async function runBroadStructureProbes(input: {
  probeQueries: string[];
  currentResults: RetrieveForNotebooksResult[];
  sources: NotebookRetrievalSource[];
  degradedSourceIds: Set<string>;
  /** plan 的 scopeLevel；null/缺省（无 planner）时保守跳过 section 探测。 */
  scopeLevel: string | null;
  retrieve: KnowledgeInjectorDeps["retrieve"];
}): Promise<BroadProbeOutcome> {
  const outcome: BroadProbeOutcome = {
    results: [],
    failures: [],
    noEvidenceSources: [],
    sectionNoEvidence: [],
    secondaryRetrievalCount: 0,
    capped: false,
  };
  const hitSourceIds = new Set<string>();
  const hitSectionsBySource = new Map<string, Set<string>>();
  // 探测途中报告索引/解析不可用的源：其零命中不能记成 no relevant evidence
  // （降级原因已由同轮 degraded 清单留痕，不冒充"无相关证据"结论）。
  const probeDegradedSourceIds = new Set<string>();
  const collectProbeDegraded = (result: RetrieveForNotebooksResult) => {
    for (const entry of result.degraded ?? []) {
      if (entry.sourceId) probeDegradedSourceIds.add(entry.sourceId);
    }
  };
  const collectHits = (chunks: NotebookRetrievalChunk[]) => {
    for (const chunk of chunks) {
      hitSourceIds.add(chunk.sourceId);
      const key = knowledgeSectionKeyOf(chunk.headingPath);
      if (key == null) continue;
      const sections = hitSectionsBySource.get(chunk.sourceId) ?? new Set<string>();
      sections.add(key);
      hitSectionsBySource.set(chunk.sourceId, sections);
    }
  };
  for (const result of input.currentResults) collectHits(result.candidates);

  // ① Source Coverage Floor：零命中 ready 源 × 全部查询（并行、bounded）。
  const zeroHitSources = input.sources.filter(source =>
    !hitSourceIds.has(source.sourceId) && !input.degradedSourceIds.has(source.sourceId));
  for (const source of zeroHitSources) {
    if (outcome.secondaryRetrievalCount >= KNOWLEDGE_SECONDARY_RETRIEVAL_MAX) {
      outcome.capped = true;
      break;
    }
    const budgetLeft = KNOWLEDGE_SECONDARY_RETRIEVAL_MAX - outcome.secondaryRetrievalCount;
    const queries = input.probeQueries.slice(0, budgetLeft);
    if (queries.length === 0) {
      outcome.capped = true;
      break;
    }
    const fullyProbed = queries.length === input.probeQueries.length;
    if (!fullyProbed) outcome.capped = true;
    outcome.secondaryRetrievalCount += queries.length;
    const settled = await Promise.allSettled(queries.map(query =>
      (async () => input.retrieve({ query, sourceIds: [source.sourceId] }))()));
    let hit = false;
    for (const entry of settled) {
      if (entry.status === "fulfilled") {
        outcome.results.push(entry.value);
        collectProbeDegraded(entry.value);
        if (entry.value.candidates.length > 0) hit = true;
        collectHits(entry.value.candidates);
      } else {
        outcome.failures.push(describeError(entry.reason));
      }
    }
    // 探测未受预算削减且仍零命中 → 如实记录（禁硬塞低质 chunk 冒充覆盖）；
    // 探测途中降级（索引缺失等）的源不作此结论。
    if (!hit && fullyProbed && !probeDegradedSourceIds.has(source.sourceId)) {
      outcome.noEvidenceSources.push(source);
    }
  }

  // ② Section Coverage：整体性 scope 下，命中章节明显不足的源补探未命中章节。
  if (input.scopeLevel != null && input.scopeLevel !== "local") {
    for (const source of input.sources) {
      if (!hitSourceIds.has(source.sourceId)) continue;
      const available = source.sections;
      if (!available || available.length < 2) continue;
      const hitSections = hitSectionsBySource.get(source.sourceId) ?? new Set<string>();
      if (hitSections.size >= available.length * KNOWLEDGE_SECTION_COVERAGE_MIN_HIT_RATIO) continue;
      const missed = available.filter(section => !hitSections.has(section));
      if (missed.length === 0) continue;
      if (outcome.secondaryRetrievalCount >= KNOWLEDGE_SECONDARY_RETRIEVAL_MAX) {
        outcome.capped = true;
        break;
      }
      const budgetLeft = KNOWLEDGE_SECONDARY_RETRIEVAL_MAX - outcome.secondaryRetrievalCount;
      const queries = input.probeQueries.slice(0, budgetLeft);
      if (queries.length === 0) {
        outcome.capped = true;
        break;
      }
      outcome.secondaryRetrievalCount += queries.length;
      const settled = await Promise.allSettled(queries.map(query =>
        (async () => input.retrieve({
          query,
          sourceIds: [source.sourceId],
          sectionsBySourceId: new Map([[source.sourceId, missed]]),
        }))()));
      const sectionsBefore = hitSectionsBySource.get(source.sourceId)?.size ?? 0;
      for (const entry of settled) {
        if (entry.status === "fulfilled") {
          outcome.results.push(entry.value);
          collectProbeDegraded(entry.value);
          collectHits(entry.value.candidates);
        } else {
          outcome.failures.push(describeError(entry.reason));
        }
      }
      const sectionsAfter = hitSectionsBySource.get(source.sourceId)?.size ?? 0;
      if (sectionsAfter === sectionsBefore && !probeDegradedSourceIds.has(source.sourceId)) {
        outcome.sectionNoEvidence.push({ source, sections: missed });
      }
    }
  }
  return outcome;
}

/**
 * 编排入口（Phase 8 执行侧升级；2026-08-31 两档化 + 滚动注入）：
 *
 * 高召回档（§三十三/§三十四，= 现状增强）：
 * 直检（原问题，与拆解并行，Recall Safety Net）+ 语义拆解子查询（≤4）+
 * 受控查询扩展（≤3，§三十五）并行检索 → 跨查询 RRF（fusionBudget 封顶）→
 * 证据组装（邻接扩展 §三十六，contextOnly）→ 预算内全量注入 / 超预算滚动
 * 注入 / 滚动不可用时截断 + 分片清单。
 *
 * broad 档（§三十七~§三十九，plan.coverageMode='broad' 或 §四十一 自动升级触发）：
 * 在高召回档检索结果之上做结构覆盖补探测——零命中源 source-constrained 二次
 * 检索（全部查询重试；仍无结果如实记录 no relevant evidence，绝不硬塞低质
 * chunk）→ 整体性 scope 下命中章节不足的源 section-constrained 二次检索 →
 * 结果并入融合。二次检索受 KNOWLEDGE_SECONDARY_RETRIEVAL_MAX 约束并全程计数。
 *
 * 自动升级（§四十一 执行侧）：high_recall 执行后 sourceCoverageFootprint 低于
 * 阈值且多源 scope → 复用已检索结果只补缺失探测（stats.upgradedTo='broad'）。
 *
 * 并行结构（降时延）：原问题直检在拆解 LLM 往返期间就已开跑——慢拆解
 * （模型慢 / 15s 超时降级）不再串行阻塞首条检索结果。拆解完成后仅对
 * 「与原问题字面不同」的子查询补检索；与原问题相同的子查询（含降级
 * 单查询路径）直接复用直检结果，不重复检索。直检与子查询结果一并进
 * RRF 融合；stats 的 subQueries/subQueryHits 仍严格对齐拆解子查询
 * （直检是并行兜底通道，不占子查询语义位，其失败照常进失败清单——
 * 显式留痕，禁静默）。
 */
export async function buildKnowledgeContextInjection(input: {
  question: string;
  mode: KnowledgeReferenceMode;
  deps: KnowledgeInjectorDeps;
  budgetTokens?: number;
  /** KnowledgeTurnScope id（Phase 4）：透传进块头与 stats；null = 无 scope（兼容旧调用方）。 */
  scopeId?: string | null;
  /**
   * 覆盖计划（Phase 7 起判定，Phase 8 起消费）：已判定或仍在判定的 plan。
   * 传 Promise 时（engine 侧 planner 与直检并行启动）在拆解前 await——保持
   * "planner 先于 decompose"（§二十九）同时不让直检安全网（§三十四）等一次
   * 分类往返。coverageMode 决定执行档位（broad→结构探测、high_recall→增强档
   * + footprint 自动升级；存量旧值 exhaustive 按 broad 执行）。
   */
  coveragePlan?: KnowledgeCoveragePlan | null
    | Promise<KnowledgeCoveragePlan | null | undefined>;
  /** §三十六 邻接扩展窗口覆写（默认 KNOWLEDGE_NEIGHBOR_EXPANSION_WINDOW；0 = 关闭）。 */
  neighborWindow?: number;
}): Promise<{ block: string; stats: KnowledgeRetrievalStats; evidence: KnowledgeInjectionEvidence }> {
  const buildStartedAt = Date.now();
  const questionTrimmed = input.question.trim();
  // ── 快速档（2026-08-31 两档化）──
  // 零辅助 LLM 轮：不拆解、不扩展、无 gap 二轮、无结构探测（engine 侧同步
  // 跳过 coverage planner 启动）；直检独走 + rerank 门控；锚点/渲染预算硬
  // 封顶；证据超封顶走截断留痕（绝不触发滚动消化——那是详细档的路径）。
  const isFast = input.mode === "fast";
  const fastRerankPolicy = { marginGate: true, deadlineMs: KNOWLEDGE_FAST_RERANK_DEADLINE_MS };
  // 直检通道立即启动（async 包裹把 retrieve 的同步抛错也归一为 rejection）。
  const directPromise = (async () => input.deps.retrieve({
    query: questionTrimmed,
    ...(isFast ? { rerankPolicy: fastRerankPolicy } : {}),
  }))();
  // 覆盖计划先行落定（planner 与直检并行；先于拆解——档位决定执行分派）。
  // 快速档不消费 plan；防御性吞掉已传入 promise 的 rejection（engine 正常
  // 不会在快速档传 plan，plan promise 本身也不 reject——见 engine 注释）。
  if (isFast && input.coveragePlan != null) {
    void Promise.resolve(input.coveragePlan).catch(() => {});
  }
  const coveragePlan = !isFast && input.coveragePlan != null ? await input.coveragePlan : null;
  // 快速档合成拆解结果：零子查询（直检即全部）、未做复杂度评估（complexity
  // 置 null——stats 不携带 decompositionComplexity，不冒充评估结论）。
  const decomposition: DecomposeResult = isFast
    ? {
      subQueries: [],
      intent: null,
      degraded: false,
      degradeReason: null,
      degradeDetail: null,
      latencyMs: 0,
      attempts: 0,
      exclusions: [],
      complexity: null,
      specialists: [],
      specialistFailures: [],
    }
    : await decomposeQuestionAdaptive({
      question: input.question,
      callModel: input.deps.decomposeModel,
    });
  // 与原问题字面相同的子查询复用直检结果；其余子查询立即并行检索。
  // parseQuestionDecomposition 已按 trimmed 去重，等值子查询至多一条。
  const isDirect = decomposition.subQueries.map(query => query.trim() === questionTrimmed);
  // ── 候选总预算（§二十一）：查询数不再隐式放大下游成本。每查询 topK =
  // 总预算对（非等值子查询 + 直检）的分摊，夹在 [24,60]；扩展查询沿用同一
  // 分摊（直检在 t0 已按自然上限 60 启动，不追溯）。
  const nonDirectSubQueryCount = isDirect.filter(equal => !equal).length;
  const perQueryTopK = Math.max(
    KNOWLEDGE_TOTAL_CANDIDATE_BUDGET_MIN_PER_QUERY,
    Math.min(
      KNOWLEDGE_CANDIDATE_GENERATION_BUDGET,
      Math.ceil(KNOWLEDGE_TOTAL_CANDIDATE_BUDGET / Math.max(1, nonDirectSubQueryCount + 1)),
    ),
  );
  const subQuerySettledPromise: Promise<PromiseSettledResult<RetrieveForNotebooksResult>[]> = Promise.allSettled(
    decomposition.subQueries
      .filter((_, index) => !isDirect[index])
      .map(subQuery => input.deps.retrieve({ query: subQuery, topK: perQueryTopK })),
  );
  // ── 扩展与子查询批并行（§十一/§二十三 链路重排，2026-08-30）：拆解一返回
  // 就同时发 (a) 子查询检索批 (b) 扩展 LLM 调用；扩展返回后其查询立即补一批
  // 检索。消除「拆解 → 扩展 → 检索」中扩展那次串行 LLM 往返（典型 1-2s、
  // 最坏 15s）。拆解降级（单查询路径已复用直检）不扩展并留痕。
  // 扩展条件门控（§十一 Conditional LLM，P2）：simple 档（单查表问题）与
  // broad+focused（单方向浅问题）不做改写扩展——直检 + BM25/向量双通道已覆盖
  // 表达差异，跳过省一次 LLM 调用并显式留痕；compound/complex 或
  // high_recall 档照常（多方向/高覆盖值得转述召回）。
  const expansionSkipReason = isFast
    ? "fast mode — direct retrieval only"
    : decomposition.degraded
      ? null
      : decomposition.complexity === "simple"
        ? "simple lookup question — direct retrieval only"
        : coveragePlan?.coverageMode === "broad" && decomposition.complexity === "focused"
          ? "broad shallow question — single evidence direction"
          : null;
  const expansionOutcomePromise: Promise<{
    expansion: Awaited<ReturnType<typeof expandQueries>> | null;
    settled: PromiseSettledResult<RetrieveForNotebooksResult>[];
  }> = (async () => {
    if (decomposition.degraded || expansionSkipReason != null) {
      return { expansion: null, settled: [] };
    }
    const expansion = await expandQueries({
      question: input.question,
      existingQueries: decomposition.subQueries,
      callModel: input.deps.expandModel ?? null,
    });
    const queries = expansion ? expansion.expansions : [];
    const settled = await Promise.allSettled(
      queries.map(query => input.deps.retrieve({ query, topK: perQueryTopK })),
    );
    return { expansion, settled };
  })();
  const [subQuerySettled, expansionOutcome] = await Promise.all([
    subQuerySettledPromise,
    expansionOutcomePromise,
  ]);
  const expansion = expansionOutcome.expansion;
  const expansionQueries = expansion ? expansion.expansions : [];
  let directValue: RetrieveForNotebooksResult | null = null;
  let directFailure: string | null = null;
  try {
    directValue = await directPromise;
  } catch (err) {
    directFailure = describeError(err);
  }

  const retrievalResults: RetrieveForNotebooksResult[] = [];
  const retrievalFailures: string[] = [];
  const subQueryHits: number[] = [];
  const expandedQueryHits: number[] = [];
  // Query Family 归属（§八，2026-08-30 拆解优化）：family 0 = 原问题族（直检 +
  // 全部扩展转述，扩展无归属字段、是查询集的变体）；family 1..N = 各非等值
  // 子查询（独立证据需求）；结构探测结果在并入时各自领新家族号。
  const resultFamilyIds: number[] = [];
  // fulfilled 结果按来源分桶收集（遥测用：扩展的独立贡献只看扩展结果，
  // 不按 flat 下标区间——直检尾插在扩展之后，区间口径会误计）。
  const nonExpansionFulfilled: RetrieveForNotebooksResult[] = [];
  const expansionFulfilled: RetrieveForNotebooksResult[] = [];
  const FAMILY_ORIGINAL = 0;
  // 结构探测结果并入时各自领新家族号（每次探测 = 一个定向补证需求）。
  let nextFamilyId = 1;
  let subQuerySettledIndex = 0;
  let nonDirectFamilySeq = 0;
  let directUsedAsSubQuery = false;
  decomposition.subQueries.forEach((subQuery, index) => {
    if (isDirect[index]) {
      directUsedAsSubQuery = true;
      if (directValue) {
        retrievalResults.push(directValue);
        resultFamilyIds.push(FAMILY_ORIGINAL);
        nonExpansionFulfilled.push(directValue);
        subQueryHits.push(directValue.candidates.length);
      } else {
        retrievalFailures.push(directFailure || "direct retrieval failed");
        subQueryHits.push(0);
      }
      return;
    }
    const outcome = subQuerySettled[subQuerySettledIndex];
    subQuerySettledIndex += 1;
    if (outcome.status === "fulfilled") {
      retrievalResults.push(outcome.value);
      resultFamilyIds.push(1 + nonDirectFamilySeq);
      nonExpansionFulfilled.push(outcome.value);
      subQueryHits.push(outcome.value.candidates.length);
    } else {
      retrievalFailures.push(describeError(outcome.reason));
      subQueryHits.push(0);
    }
    nonDirectFamilySeq += 1;
  });
  // 扩展查询的名次序列（hits 对齐 expandedQueries）——变体并入原问题族。
  expansionOutcome.settled.forEach((outcome) => {
    if (outcome.status === "fulfilled") {
      retrievalResults.push(outcome.value);
      resultFamilyIds.push(FAMILY_ORIGINAL);
      expansionFulfilled.push(outcome.value);
      expandedQueryHits.push(outcome.value.candidates.length);
    } else {
      retrievalFailures.push(describeError(outcome.reason));
      expandedQueryHits.push(0);
    }
  });
  // 无等值子查询时，直检作为第 N+1 条名次序列并入 RRF 融合（原问题族）。
  if (!directUsedAsSubQuery) {
    if (directValue) {
      retrievalResults.push(directValue);
      resultFamilyIds.push(FAMILY_ORIGINAL);
      nonExpansionFulfilled.push(directValue);
    } else {
      retrievalFailures.push(directFailure || "direct retrieval failed");
    }
  }
  const budgetTokens = input.budgetTokens ?? KNOWLEDGE_INJECTION_FALLBACK_BUDGET_TOKENS;

  // ── 执行档位分派（Phase 8 消费 plan；2026-08-31 两档化）──
  // 存量持久化 plan 的旧值 'exhaustive' 一律按 broad 执行（结构探测 + 滚动注入
  // 在超预算时自然承担整库阅读需求）。
  let executionMode: "high_recall" | "broad";
  if (coveragePlan != null && coveragePlan.coverageMode !== "high_recall") {
    executionMode = "broad";
  } else {
    executionMode = "high_recall";
  }
  const coverageNotes: string[] = [];
  // ── Gap Analyzer 二轮补证（§二十二，P2）：条件触发一次（高覆盖模式或已知
  // 缺口），≤3 条补证查询补一波检索，各自领新家族号；最多一轮，禁静默留痕。 ──
  let gapQueries: string[] = [];
  let gapQueryHits: number[] = [];
  let secondPassTriggered = false;
  let secondPassReason: string | null = null;
  const gapAnalysisModel = !isFast ? input.deps.gapAnalysisModel ?? null : null;
  if (gapAnalysisModel != null) {
    const gapGate = shouldRunGapAnalysis({
      coverageMode: coveragePlan?.coverageMode ?? "high_recall",
      subQueries: decomposition.subQueries,
      subQueryHits,
      originalQueryHits: directValue ? directValue.candidates.length : 0,
    });
    if (gapGate.trigger) {
      const gapOutcome = await runGapAnalysis({
        question: input.question,
        existing: [
          ...(directValue
            ? [{ query: questionTrimmed, hits: directValue.candidates.length }]
            : []),
          ...decomposition.subQueries.map((query, index) => ({ query, hits: subQueryHits[index] ?? 0 })),
          ...expansionQueries.map((query, index) => ({ query, hits: expandedQueryHits[index] ?? 0 })),
        ],
        callModel: gapAnalysisModel,
      });
      if (gapOutcome.degraded && gapOutcome.degradeReason) {
        coverageNotes.push(`[gap analysis unavailable: ${gapOutcome.degradeReason}]`);
      }
      if (gapOutcome.queries.length > 0) {
        secondPassTriggered = true;
        secondPassReason = gapGate.reason;
        gapQueries = gapOutcome.queries;
        const gapSettled = await Promise.allSettled(
          gapQueries.map(query => input.deps.retrieve({ query, topK: perQueryTopK })),
        );
        gapSettled.forEach((outcome) => {
          const familyId = nextFamilyId;
          nextFamilyId += 1;
          if (outcome.status === "fulfilled") {
            retrievalResults.push(outcome.value);
            resultFamilyIds.push(familyId);
            gapQueryHits.push(outcome.value.candidates.length);
          } else {
            retrievalFailures.push(describeError(outcome.reason));
            gapQueryHits.push(0);
          }
        });
        coverageNotes.push(
          `[gap analysis second pass: ${gapQueries.length} supplemental evidence quer${gapQueries.length === 1 ? "y" : "ies"} (${gapGate.reason})]`,
        );
      }
    }
  }

  // 扩展留痕：成功列出采纳的扩展查询；不可用/失败显式标注（禁静默降级）。
  const expansionAnnotation = expansion == null
    ? []
    : expansion.degraded
      ? [`[query expansion unavailable: ${expansion.degradeReason}]`]
      : expansion.expansions.length > 0
        ? [
          "Query expansions (controlled):",
          ...expansion.expansions.map(query => `- ${query}`),
        ]
        : [];

  let finalResults = retrievalResults;
  let finalFamilyIds = resultFamilyIds;
  const mergedFailures = [...retrievalFailures];
  let noEvidenceSources: NotebookRetrievalSource[] = [];
  let sectionNoEvidence: Array<{ source: NotebookRetrievalSource; sections: string[] }> = [];
  let secondaryRetrievalCount = 0;
  let secondaryCapped = false;
  let upgradedTo: "broad" | undefined;
  const allSources = mergeSources(retrievalResults);
  // 家族分组（§八）：两级融合与边际收益统计共用 groupFamiliesById；探测并入后
  // 以 finalResults/finalFamilyIds 重算。
  // 融合池上限随预算倒推（70% 折算块数；按初轮候选预估，探测后以 finalResults
  // 重算并用于正式融合）。
  const fusionPoolBudgetPreview = resolveFusionPoolBudget({
    budgetTokens,
    candidates: retrievalResults.flatMap(result => result.candidates),
  });
  // broad 结构探测用的查询集：直检 + 子查询 + 扩展（去重、保序）。
  const probeQueries = [...new Set([
    questionTrimmed,
    ...decomposition.subQueries.map(query => query.trim()),
    ...expansionQueries,
  ])];
  const runProbes = async () => {
    const probes = await runBroadStructureProbes({
      probeQueries,
      currentResults: retrievalResults,
      sources: allSources,
      degradedSourceIds: degradedSourceIdsOf(retrievalResults),
      scopeLevel: coveragePlan?.scopeLevel ?? null,
      retrieve: input.deps.retrieve,
    });
    if (probes.results.length > 0) {
      // 每次探测结果 = 一个定向补证需求，各自领新家族号（§八 两级融合下
      // 与既有证据需求等权一票）。
      finalResults = [...finalResults, ...probes.results];
      finalFamilyIds = [
        ...finalFamilyIds,
        ...probes.results.map(() => {
          const familyId = nextFamilyId;
          nextFamilyId += 1;
          return familyId;
        }),
      ];
    }
    mergedFailures.push(...probes.failures);
    noEvidenceSources = probes.noEvidenceSources;
    sectionNoEvidence = probes.sectionNoEvidence;
    secondaryRetrievalCount = probes.secondaryRetrievalCount;
    secondaryCapped = probes.capped;
    if (probes.capped) {
      coverageNotes.push(
        `[coverage probing incomplete: secondary retrieval budget (${KNOWLEDGE_SECONDARY_RETRIEVAL_MAX} calls) exhausted]`,
      );
    }
  };
  if (!isFast && executionMode === "broad") {
    await runProbes();
  } else if (!isFast && coveragePlan && coveragePlan.coverageMode === "high_recall") {
    // §四十一 执行侧自动升级：主轮 footprint 不足且多源 scope → 复用已检索
    // 结果，只补 broad 的缺失探测（不重跑已命中的部分）。
    const previewFootprint = computeCoverageFootprint({
      fused: fuseQueryFamilies(groupFamiliesById(retrievalResults, resultFamilyIds), fusionPoolBudgetPreview),
      sources: allSources,
      candidateChunkCount: retrievalResults.reduce((sum, result) => sum + result.candidates.length, 0),
    });
    if (
      previewFootprint.selectedSourceCount >= KNOWLEDGE_AUTO_UPGRADE_MIN_SOURCES
      && previewFootprint.sourceCoverageFootprint != null
      && previewFootprint.sourceCoverageFootprint < KNOWLEDGE_AUTO_UPGRADE_SOURCE_FOOTPRINT_MIN
    ) {
      upgradedTo = "broad";
      // 实际执行档位随之改为 broad（stats.executedCoverageMode 如实反映补轮）。
      executionMode = "broad";
      coverageNotes.push(
        `[coverage auto-upgrade: high_recall → broad `
        + `(source coverage footprint ${roundFootprint(previewFootprint.sourceCoverageFootprint)} `
        + `below ${KNOWLEDGE_AUTO_UPGRADE_SOURCE_FOOTPRINT_MIN})]`,
      );
      await runProbes();
    }
  }

  // ── 融合 → 证据组装（§二十六 预算链 + §三十六 邻接扩展）──
  // 融合池上限（阀 A）与锚点配额（阀 B）同源随预算倒推：池 70% 候选水位、
  // 锚 50% 装填配额（池略高留选择余量）。
  const fusionPoolBudget = resolveFusionPoolBudget({
    budgetTokens,
    candidates: finalResults.flatMap(result => result.candidates),
  });
  let fused = fuseQueryFamilies(groupFamiliesById(finalResults, finalFamilyIds), fusionPoolBudget);
  // 否定排除（§九，P2）：词法约束在融合后生效（锚点之前），过度匹配保护。
  let negationDroppedChunks = 0;
  let negationFilterSkipped = false;
  if (decomposition.exclusions.length > 0) {
    const filtered = applyNegationExclusions({ chunks: fused, exclusions: decomposition.exclusions });
    fused = filtered.kept;
    negationDroppedChunks = filtered.droppedCount;
    negationFilterSkipped = filtered.skipped;
    if (filtered.droppedCount > 0) {
      coverageNotes.push(
        `[negation exclusion: dropped ${filtered.droppedCount} chunks matching excluded terms]`,
      );
    } else if (filtered.skipped) {
      coverageNotes.push(
        "[negation exclusion skipped: excluded terms match more than half the pool — suspected lexical collision]",
      );
    }
  }
  // 锚点上限随注入预算伸缩（大上下文模型多带证据，小模型维持既有 40 兜底）。
  // 快速档再叠加硬封顶（12 条高命中头部证据，不随窗口放大）。
  const anchorBudget = resolveEvidenceAnchorBudget({ budgetTokens, fused });
  const effectiveAnchorCap = isFast
    ? Math.min(KNOWLEDGE_FAST_MAX_EVIDENCE_ENTRIES, anchorBudget)
    : anchorBudget;
  const anchors = fused.slice(0, effectiveAnchorCap);
  const candidateChunkCount = finalResults.reduce((sum, result) => sum + result.candidates.length, 0);
  const footprint = computeCoverageFootprint({ fused, sources: allSources, candidateChunkCount });

  if (fused.length > anchors.length) {
    coverageNotes.push(
      `(${fused.length - anchors.length} fused candidates beyond the evidence budget `
      + `(${effectiveAnchorCap}) were not assembled into evidence)`,
    );
  }
  const assembleStart = Date.now();
  const evidence = assembleEvidenceEntries({
    anchors,
    window: input.neighborWindow ?? KNOWLEDGE_NEIGHBOR_EXPANSION_WINDOW,
    readNeighborChunks: input.deps.readNeighborChunks ?? null,
  });
  const executionStats: KnowledgeExecutionStats = {
    ...(coveragePlan ? { executedCoverageMode: executionMode } : {}),
    ...(upgradedTo ? { upgradedTo } : {}),
    ...(expansion
      ? {
        expandedQueries: expansionQueries,
        expandedQueryHits,
        ...(expansion.degraded && expansion.degradeReason
          ? { expansionDegradeReason: expansion.degradeReason }
          : {}),
      }
      : {}),
    selectedSourceCount: footprint.selectedSourceCount,
    retrievedSourceCount: footprint.retrievedSourceCount,
    availableSectionCount: footprint.availableSectionCount,
    retrievedSectionCount: footprint.retrievedSectionCount,
    candidateChunkCount: footprint.candidateChunkCount,
    uniqueChunkCount: footprint.uniqueChunkCount,
    secondaryRetrievalCount,
    ...(secondaryCapped ? { secondaryRetrievalCapped: true } : {}),
    ...(footprint.sourceCoverageFootprint != null
      ? { sourceCoverageFootprint: roundFootprint(footprint.sourceCoverageFootprint) }
      : {}),
    ...(footprint.sectionCoverageFootprint != null
      ? { sectionCoverageFootprint: roundFootprint(footprint.sectionCoverageFootprint) }
      : {}),
    ...(footprint.chunkRecallFootprint != null
      ? { chunkRecallFootprint: roundFootprint(footprint.chunkRecallFootprint) }
      : {}),
    // ── P2 拆解优化统计（§四/§十一/§九/§二十二）──
    ...(decomposition.complexity != null
      ? { decompositionComplexity: decomposition.complexity }
      : {}),
    ...(decomposition.specialists.length > 0
      ? { decompositionSpecialists: [...decomposition.specialists] }
      : {}),
    ...(decomposition.specialistFailures.length > 0
      ? { decompositionSpecialistFailures: [...decomposition.specialistFailures] }
      : {}),
    ...(expansionSkipReason != null ? { expansionSkipReason } : {}),
    ...(decomposition.exclusions.length > 0
      ? { negationExclusions: [...decomposition.exclusions] }
      : {}),
    ...(negationDroppedChunks > 0 ? { negationDroppedChunks } : {}),
    ...(negationFilterSkipped ? { negationFilterSkipped: true } : {}),
    ...(secondPassTriggered
      ? {
        secondPassTriggered: true,
        ...(secondPassReason != null ? { secondPassReason } : {}),
        gapQueries: [...gapQueries],
        gapQueryHits: [...gapQueryHits],
      }
      : {}),
  };
  const notes = [...coverageNotes, ...expansionAnnotation];

  // ── 检索遥测（§二十五，2026-08-30 拆解优化）：家族级边际收益与重叠率 ──
  // 按 finalResults/finalFamilyIds 的最终态计算（探测并入后）。
  const totalCandidateRefs = finalResults.reduce((sum, result) => sum + result.candidates.length, 0);
  const uniqueCandidateIds = new Set(
    finalResults.flatMap(result => result.candidates.map(chunk => chunk.id)),
  ).size;
  const telemetryFamilySequence = groupFamiliesById(finalResults, finalFamilyIds);
  const telemetrySeenIds = new Set<string>();
  const evidenceNeedGains = telemetryFamilySequence.map(family => {
    const ranking = rrfFuseRankings(family.map(result => result.candidates), Number.MAX_SAFE_INTEGER);
    let gain = 0;
    for (const chunk of ranking) {
      if (!telemetrySeenIds.has(chunk.id)) {
        telemetrySeenIds.add(chunk.id);
        gain += 1;
      }
    }
    return gain;
  });
  const nonExpansionIds = new Set(
    nonExpansionFulfilled.flatMap(result => result.candidates.map(chunk => chunk.id)),
  );
  const expansionNewIds = new Set<string>();
  for (const result of expansionFulfilled) {
    for (const chunk of result.candidates) {
      if (!nonExpansionIds.has(chunk.id) && !expansionNewIds.has(chunk.id)) {
        expansionNewIds.add(chunk.id);
      }
    }
  }
  const retrievalTelemetry = {
    originalQueryHits: directValue ? directValue.candidates.length : 0,
    expansionUniqueHits: expansionNewIds.size,
    queryOverlapRatio: totalCandidateRefs > 0
      ? Math.round((1 - uniqueCandidateIds / totalCandidateRefs) * 1000) / 1000
      : 0,
    evidenceNeedGains,
  };

  // ── 超预算滚动注入（2026-08-31，取代蒸馏压缩）──
  // 证据总量超出注入预算时：装填序列（锚点 + contextOnly 邻接块）按预算拆成
  // N 份，前 N-1 份由会话主模型逐份消化成中间笔记（knowledge-rollup；循环内
  // 模型可用 need-more-evidence fenced 块自主发起补充检索），最后一部分与全部
  // 中间笔记进入最终注入块，由正常 session.prompt 轮产出用户可见答案。
  // 滚动不可用/失败 → 降级预算截断 + 分片清单路径并留痕（禁静默）。
  // [KN] 全局编号在此预渲染定死（跨部分连续、与最终块一致）。
  const renderedEntries: KnowledgeRollupEntry[] = [];
  {
    let lastAnchorLabel: number | null = null;
    let labelCursor = 0;
    for (const entry of evidence) {
      labelCursor += 1;
      const header = chunkHeader(
        entry.chunk,
        labelCursor - 1,
        entry.contextOnly && lastAnchorLabel != null ? lastAnchorLabel - 1 : undefined,
      );
      const body = quoteText(entry.chunk.text);
      renderedEntries.push({
        chunk: entry.chunk,
        contextOnly: entry.contextOnly,
        labelIndex: labelCursor,
        anchorLabelIndex: entry.contextOnly && lastAnchorLabel != null ? lastAnchorLabel : labelCursor,
        text: `${header}\n${body}`,
      });
      if (!entry.contextOnly) lastAnchorLabel = labelCursor;
    }
  }
  const totalCost = renderedEntries.reduce(
    (sum, entry) => sum + estimateTextTokens(markScannedEvidence(entry.text)),
    0,
  );
  const assembleMs = Date.now() - assembleStart;
  // 快速档渲染预算收紧（只影响装填循环与 stats 口径；外层 budgetTokens 不动，
  // 避免把快速档推进滚动判定）。
  const renderBudgetTokens = isFast
    ? Math.min(budgetTokens, KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS)
    : budgetTokens;
  let rollupPayload: {
    digests: Array<{ partIndex: number; notes: string }>;
    finalEntries: KnowledgeRollupEntry[];
    allEntries: KnowledgeRollupEntry[];
  } | null = null;
  let rollupDegradedReason: string | undefined;
  let rollupStart: number | null = null;
  if (!isFast && totalCost > budgetTokens && renderedEntries.length > 0 && input.deps.rollupModel) {
    rollupStart = Date.now();
    const rollup = await runKnowledgeRollup({
      question: input.question,
      entries: renderedEntries,
      budgetTokens,
      deps: {
        rollupModel: input.deps.rollupModel,
        retrieve: input.deps.retrieve,
        ...(input.deps.signal ? { signal: input.deps.signal } : {}),
        ...(input.deps.onRollupProgress ? { onProgress: input.deps.onRollupProgress } : {}),
        ...(input.deps.onSupplementalSearch
          ? { onSupplementalSearch: input.deps.onSupplementalSearch }
          : {}),
        ...(input.deps.recentTurnsExcerpt != null
          ? { recentTurnsExcerpt: input.deps.recentTurnsExcerpt }
          : {}),
      },
    });
    if (rollup.ok === true) {
      rollupPayload = rollup.result;
      executionStats.rollupParts = rollup.result.stats.parts;
      executionStats.rollupRounds = rollup.result.stats.rounds;
      if (rollup.result.stats.supplementalQueries.length > 0) {
        executionStats.rollupSupplementalQueries = [...rollup.result.stats.supplementalQueries];
      }
      if (rollup.result.stats.degradedReason) {
        executionStats.rollupDegradedReason = rollup.result.stats.degradedReason;
      }
    } else {
      rollupDegradedReason = rollup.reason;
      executionStats.rollupDegradedReason = rollup.reason;
    }
  } else if (isFast && totalCost > renderBudgetTokens && renderedEntries.length > 0) {
    // 快速档证据超封顶：滚动消化禁用，截断路径显式留痕（禁静默；渲染层统一
    // 追加 "; budget truncation applied" 后缀）。
    rollupDegradedReason = "fast mode: rolling digest disabled";
    executionStats.rollupDegradedReason = rollupDegradedReason;
  } else if (totalCost > budgetTokens && renderedEntries.length > 0) {
    rollupDegradedReason = "rollup model not configured";
    executionStats.rollupDegradedReason = rollupDegradedReason;
  }
  // ── 分段计时汇总（2026-08-31 观测补齐）：检索段取跨查询/跨笔记本最大值
  // （并行批里最慢的才是关键路径），编排层补 planner/assemble/rollup/total。──
  const rollupMs = rollupStart != null ? Date.now() - rollupStart : undefined;
  const stageTimingsPayload: NonNullable<KnowledgeRetrievalStats["stageTimings"]> = {};
  for (const result of finalResults) {
    if (!result.stageTimings) continue;
    for (const [key, value] of Object.entries(result.stageTimings)) {
      if (typeof value !== "number") continue;
      if (value > ((stageTimingsPayload as Record<string, number | undefined>)[key] ?? 0)) {
        (stageTimingsPayload as Record<string, number | undefined>)[key] = value;
      }
    }
  }
  stageTimingsPayload.plannerMs = decomposition.latencyMs;
  stageTimingsPayload.assembleMs = assembleMs;
  if (rollupMs != null) stageTimingsPayload.rollupMs = rollupMs;
  stageTimingsPayload.totalMs = Date.now() - buildStartedAt;
  return renderKnowledgeContextBlock({
    mode: input.mode,
    decomposition,
    retrievalResults: finalResults,
    retrievalFailures: mergedFailures,
    subQueryHits,
    retrievalFamilyIds: finalFamilyIds,
    retrievalTelemetry,
    stageTimings: stageTimingsPayload,
    budgetTokens: renderBudgetTokens,
    ...(input.scopeId ? { scopeId: input.scopeId } : {}),
    ...(coveragePlan ? { coveragePlan } : {}),
    ...(rollupPayload ? { rollup: rollupPayload } : {}),
    ...(rollupDegradedReason ? { rollupDegradedReason } : {}),
    evidence,
    coverageNotes: notes,
    noEvidenceSources,
    sectionNoEvidence,
    executionStats,
  });
}

/** footprint 数值留痕统一 4 位小数（展示口径，不改变计算精度语义）。 */
function roundFootprint(value: number): number {
  return Math.round(value * 10_000) / 10_000;
}
