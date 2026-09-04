/**
 * 知识库引用（knowledgeRefs）——聊天消息携带的笔记本级引用契约。
 *
 * 引用在会话内持续生效直到手动取消；前端每条消息显式携带（服务端无状态，
 * 与 sessionFileRefs 同模式）：
 *   wsMsg.knowledgeRefs = { notebookIds: string[], mode: "fast" | "detailed" }
 *
 * Phase 7 只做透传 + 严格校验；Phase 8 的 knowledge-context-injector 在
 * core/desktop-session-submit.ts 的 prompt 组装区消费它做拆解与检索注入。
 */

import type { KnowledgeDegradeReason } from "./knowledge-reason-codes.ts";
import type { KnowledgeCompletenessPolicy } from "./knowledge-execution.ts";

/**
 * 答案模式两档（2026-08-31 两档化，取代旧 qa/assist）：
 * - fast：零辅助 LLM 轮（不拆解）、rerank 动态门控、证据注入硬封顶、不触发
 *   滚动消化——以最快速度给出高命中回答；
 * - detailed：自适应拆解 + coverage 两档 + 超预算滚动消化的全量召回路径
 *   （两档化前的既有行为，作为回归锚）。
 */
export type KnowledgeReferenceMode = "fast" | "detailed";

/**
 * 存量答案模式（两档化前的 qa/assist）：生产写入侧不再产出。读取侧（历史
 * 还原 / retry-fork 重放）经 normalizeLegacyKnowledgeReferenceMode 一律映射
 * detailed（行为保持，沿用 coverage exhaustive→broad 的存量兼容先例）；显示层
 * 保留原值渲染旧标签。
 */
export type LegacyKnowledgeReferenceMode = "qa" | "assist";

/**
 * 存量值读取侧归一：fast/detailed 原样返回；qa/assist → detailed；非法值 →
 * null（调用方显式处理，禁静默）。
 */
export function normalizeLegacyKnowledgeReferenceMode(mode: string): KnowledgeReferenceMode | null {
  if (mode === "fast" || mode === "detailed") return mode;
  if (mode === "qa" || mode === "assist") return "detailed";
  return null;
}

/**
 * Coverage 维度的档位枚举（任务书 §二十八 三维度正交，Phase 7；2026-08-31
 * 两档化）：与 answerMode（fast/detailed）、retrievalMode（fts/hybrid）互不
 * 携带、互不影响。定义在 shared 层供 stats 契约与 lib/knowledge 的 planner
 * 共用（lib → shared 单向）。
 *
 * 'exhaustive' 值仅作存量兼容：旧持久化 plan 行 / 旧会话 stats 可能携带，
 * 生产写入侧不再产出；执行侧读到旧值一律按 broad 处理。
 */
export type KnowledgeCoverageMode = "high_recall" | "broad" | "exhaustive";

/** plan 判定的目标范围层级（任务书 §三十）。 */
export type KnowledgeCoverageScopeLevel =
  | "local"
  | "source"
  | "multi_source"
  | "notebook"
  | "multi_notebook"
  | "whole_scope";

export interface KnowledgeRefs {
  notebookIds: string[];
  mode: KnowledgeReferenceMode;
}

/**
 * 一次降级留痕的 per-scope 明细（Phase 2 Query/Index 分离）：被引笔记本内某个
 * 源/索引变体本轮不可用（索引未建/在建/失败、向量未就绪、源需 OCR），检索
 * 照常进行（其余就绪 scope 不受影响），缺失部分已幂等入队后台构建。
 */
export interface KnowledgeDegradedScope {
  reason: KnowledgeDegradeReason;
  notebookId?: string;
  notebookName?: string;
  sourceId?: string;
  sourceName?: string;
  parseArtifactId?: string;
}

/**
 * 一次知识库注入的检索统计（冻结契约）：随注入块一起产出，经
 * desktop-session-submit 持久化到 presentation 条目（knowledgeRetrieval 字段）
 * 并随 session_user_message / 历史合并透出，供前端渲染「检索 N 块 · 注入 M 块」。
 */
export interface KnowledgeRetrievalStats {
  mode: KnowledgeReferenceMode;
  /** 缺省表示旧消息；展示层不得把旧快速模式统计冒充纯本地检索。 */
  executionPath?: "fast_local" | "detailed_research";
  deadlineMs?: number;
  deadlineExceeded?: boolean;
  remoteModelCalls?: number;
  ftsQueries?: number;
  vectorQueries?: number;
  rerankCalls?: number;
  scopeCompileMs?: number;
  timeToFirstEvidenceMs?: number;
  vectorBackend?: "hnsw" | "portable" | "none";
  searchCalls?: number;
  readCalls?: number;
  grepCalls?: number;
  /** 只保存结构化研究状态，不保存模型原始思考。 */
  research?: {
    runId: string;
    status: "planning" | "running" | "synthesizing" | "completed" | "partial" | "failed" | "cancelled";
    completenessPolicy: KnowledgeCompletenessPolicy;
    rounds: number;
    toolCalls: number;
    delegatedAgents: number;
    needsTotal: number;
    needsSupported: number;
    needsPartial: number;
    needsConflicted: number;
    unresolvedNeedIds: string[];
    stopReason: string;
  };
  /**
   * KnowledgeTurnScope id（Phase 4）：本轮知识权限天花板的服务端实体 id，
   * 随注入块头一起产出；模型调 knowledge_read 必须回传。仅会话注入路径携带。
   */
  scopeId?: string;
  retrievalMode: "hybrid" | "fts" | "none";
  /**
   * Phase 2：本轮请求的检索模式（任一被引笔记本配置了可路由的嵌入模型即
   * "hybrid"）。retrievalMode（实际使用）因向量变体未就绪而低于请求值时，
   * degradedScopes 携带逐 scope 的显式原因（§十二）。
   */
  retrievalModeRequested?: "hybrid" | "fts";
  /** Phase 2：逐 scope 降级明细（去重后）；空数组/缺省 = 无降级。 */
  degradedScopes?: KnowledgeDegradedScope[];
  subQueries: string[];
  /** 每条子查询的候选命中数（与 subQueries 下标对齐；失败记 0）。 */
  subQueryHits: number[];
  /** 拆解降级（单查询 + 注入块内留痕）。 */
  degraded: boolean;
  degradeReason?: string;
  /**
   * 拆解遥测（2026-08-30 拆解优化 §二十五）：latencyMs 含纠错重试；retryCount
   * = 纠错次数（0=首跑采纳）。缺省 = 旧调用方未接入。
   */
  decompositionLatencyMs?: number;
  decompositionRetryCount?: number;
  /**
   * 检索遥测（§二十五，家族级）：originalQueryHits = 直检命中数；
   * expansionUniqueHits = 扩展查询独立召回的新块数（subQueryMarginalGain 的
   * 扩展侧）；queryOverlapRatio = 1 − 去重后唯一块 / 总候选引用（0=无重叠）；
   * evidenceNeedGains = 每证据需求（家族）边际新增块数（家族序：原问题族在前）。
   */
  originalQueryHits?: number;
  expansionUniqueHits?: number;
  queryOverlapRatio?: number;
  evidenceNeedGains?: number[];
  /**
   * P2 拆解优化统计（2026-08-30）：复杂度档位（simple=零拆解 LLM 直检即全部）/
   * 实际执行的专业方向 / 部分方向失败留痕；扩展跳过原因（条件门控）；
   * 否定排除条件与词法过滤计数（filterSkipped=过度匹配保护触发）；
   * Gap Analyzer 二轮补证（触发原因 + 补证查询与命中）。
   */
  decompositionComplexity?: "simple" | "focused" | "compound" | "complex";
  decompositionSpecialists?: string[];
  decompositionSpecialistFailures?: string[];
  expansionSkipReason?: string;
  negationExclusions?: string[];
  negationDroppedChunks?: number;
  negationFilterSkipped?: boolean;
  secondPassTriggered?: boolean;
  secondPassReason?: string;
  gapQueries?: string[];
  gapQueryHits?: number[];
  /** 融合去重后的候选块数。 */
  fusedChunks: number;
  /** 预算内实际注入块数。 */
  injectedChunks: number;
  /** 超预算走分片清单。 */
  truncated: boolean;
  usedTokens: number;
  budgetTokens: number;
  /** 整体注入不可用原因（engine 门面 annotateUnavailable / 检索全失败路径）。 */
  unavailableReason?: string;
  /**
   * 逐条注入结果（冻结形状，供前端做「已搜索 N 个结果」展开列表）：
   * ordinal 与注入块 [KN] 编号一致；chunkOrdinal 是块在源内的 1-based 序号
   * （与 knowledge_read / 超预算分片清单同一语义）；firstLine 为块正文首行截断。
   * 仅正常注入路径产出（可为空数组）；unavailable 降级路径（engine 门面 /
   * submit 兜底）不带该字段。超预算分片时只含实际注入的块。
   */
  results?: Array<{
    ordinal: number;
    sourceName: string;
    chunkOrdinal: number;
    firstLine: string;
    /** 邻接扩展的上下文块（§三十六，Phase 8）：进入注入块但不计任何检索命中。 */
    contextOnly?: boolean;
  }>;
  /**
   * @deprecated 蒸馏压缩路径已移除（2026-08-31）；字段仅为旧会话存量 stats 的
   * 读取兼容保留，生产写入侧不再产出。
   */
  distilled?: boolean;
  /** @deprecated 同上（蒸馏批数）。 */
  distillBatches?: number;
  /** @deprecated 同上（蒸馏降级原因）。 */
  distillDegradedReason?: string;
  /**
   * 滚动注入统计（2026-08-31 取代蒸馏）：证据总量超预算时证据被拆成 N 份、
   * 由会话主模型逐部分消化。parts = 拆分总数（最后一部分直接进注入块）；
   * rounds = 实际执行的中间轮数；supplementalQueries = 循环内模型自主发起的
   * 补充检索查询全集；degradedReason = 留痕（笔记截断/轮上限触顶等，禁静默）。
   */
  rollup?: {
    parts: number;
    rounds: number;
    supplementalQueries?: string[];
    degradedReason?: string;
  };
  /**
   * rerank 降级留痕（2026-08-30 延迟加固）：任一被引笔记本的重排超期/传输失败
   * 时携带（多笔记本以 "; " 连接，保留笔记本归属），候选保持 RRF 名次，
   * 注入块内同文案留痕。缺省 = 全部正常重排或未配置重排。
   */
  rerankDegradeReason?: string;
  /**
   * rerank 动态门控跳过留痕（2026-08-31 快速档）：快速档检索结果头部清晰
   * （top-1 RRF 融合分领先 ≥ 阈值）时主动跳过重排、保持 RRF 名次。多笔记本以
   * "; " 连接保留笔记本归属。与 rerankDegradeReason 的区别：这是主动跳过
   * 而非失败降级，只进 stats 不进注入块。缺省 = 未跳过（正常重排/未配置/
   * 非快速档）。
   */
  rerankSkippedReason?: string;
  /** 实际执行的模型组数；缓存命中表示至少一个对应缓存读取命中。 */
  embeddingGroups?: number;
  rerankGroups?: number;
  queryEmbeddingCacheHit?: boolean;
  retrievalResultCacheHit?: boolean;
  /**
   * 检索分段计时（2026-08-31）：各阶段墙钟毫秒（单笔记本取该段合计，多笔记本
   * 取最大值——反映对关键路径的贡献）。纯增量可选字段，旧调用方/旧会话不携带。
   */
  stageTimings?: {
    ftsMs?: number;
    embedMs?: number;
    vectorMs?: number;
    fuseMs?: number;
    rerankMs?: number;
    plannerMs?: number;
    assembleMs?: number;
    rollupMs?: number;
    totalMs?: number;
  };
  /**
   * Coverage 维度摘要（任务书 §二十八/§二十九，Phase 7）：本轮 KnowledgeCoveragePlan
   * 的结构化结论。只携带 plan 的判定结果，不携带任何 CoT/原始模型输出；缺省 =
   * 调用方（旧调用路径/降级块）未接入 coverage planner。
   */
  coverageMode?: KnowledgeCoverageMode;
  scopeLevel?: KnowledgeCoverageScopeLevel;
  /** 规则层命中的确定性规则 id（如 RULE_EXHAUSTIVE_KEYWORD / RULE_GLOBAL_NEGATIVE）。 */
  matchedRuleIds?: string[];
  /**
   * Phase 8 执行档位（§三十三~§三十七；2026-08-31 两档化）：本轮实际执行的
   * 覆盖档位。plan 消费后与计划档位可能不同——high_recall 可被自动升级（见
   * upgradedTo）。存量旧值 'exhaustive' 的 plan 执行侧按 broad 处理。仅在接入
   * coverage planner 时携带。
   */
  executedCoverageMode?: "high_recall" | "broad";
  /**
   * §四十一（执行侧）：high_recall 执行后 sourceCoverageFootprint 低于阈值且多源
   * scope，自动补一轮 broad 流程（复用已检索结果，只补缺失探测）时标注。
   */
  upgradedTo?: "broad";
  /**
   * 受控查询扩展（§三十五，Phase 8）：实际采纳的扩展查询（去重后 ≤
   * KNOWLEDGE_QUERY_EXPANSION_MAX，与拆解子查询共用总查询预算）。仅在尝试扩展
   * （拆解成功且扩展模型可用）时携带；空数组 = 模型判定无需扩展。
   */
  expandedQueries?: string[];
  /** 扩展查询的候选命中数（与 expandedQueries 下标对齐；失败记 0）。 */
  expandedQueryHits?: number[];
  /** 扩展不可用/失败的留痕（禁静默降级；未尝试扩展时不携带）。 */
  expansionDegradeReason?: string;
  /**
   * Coverage Footprint 计数（§四十，Phase 8；普通与 broad 检索均记录，全部可选、
   * 向后兼容）。selected/retrieved 的分母分子只统计检索锚点（原始/子/扩展查询的
   * 融合命中），邻接扩展块（contextOnly）不计入任何分子。
   */
  selectedSourceCount?: number;
  /** 被引范围内有 ≥1 个检索锚点命中的 ready 源数。 */
  retrievedSourceCount?: number;
  /** 选中源内可用 section（headingPath 分桶键）总数；无结构元数据的源计 0。 */
  availableSectionCount?: number;
  /** 有 ≥1 个检索锚点命中的 section 数。 */
  retrievedSectionCount?: number;
  /** 各查询候选条目总数（跨查询未去重；预算链入口计数）。 */
  candidateChunkCount?: number;
  /** 跨查询 RRF 融合去重后的唯一 chunk 数（fusionBudget 截断后）。 */
  uniqueChunkCount?: number;
  /** 邻接扩展实际注入的上下文块数（contextOnly，不计入任何 footprint 分子）。 */
  neighborExpansionCount?: number;
  /** broad 档结构缺口探测执行的二次检索调用次数（source/section constrained）。 */
  secondaryRetrievalCount?: number;
  /** 派生：retrievedSourceCount / selectedSourceCount（分母为 0 时不携带）。 */
  sourceCoverageFootprint?: number;
  /** 派生：retrievedSectionCount / availableSectionCount（分母为 0 时不携带）。 */
  sectionCoverageFootprint?: number;
  /**
   * 派生：uniqueChunkCount / 选中源 chunk 总数。这只是「本轮检索触达了多少
   * 资料」的触达率（touched / available），不是科学意义上的召回率——绝不能在
   * 代码注释、UI 或日志中称为 actual recall（§四十）。分母为 0 时不携带。
   */
  chunkRecallFootprint?: number;
  /** 二次探测达到预算上限仍未探完（显式留痕；缺省 = 探测未受限或无缺口）。 */
  secondaryRetrievalCapped?: boolean;
}

/**
 * 归一化 + 严格校验（禁静默降级红线）：
 * - value == null → null（本条消息未引用知识库）
 * - 形状非法（notebookIds 非字符串数组、空串、mode 非 qa|assist）→ 抛 TypeError，
 *   由调用方转成显式拒绝（WS error / HTTP 400），不允许悄悄丢掉引用。
 * 合法输入返回去重后的新对象；空 notebookIds 归一为 null（等价于未引用）。
 */
export function normalizeKnowledgeRefs(value: unknown): KnowledgeRefs | null {
  if (value == null) return null;
  if (typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("knowledgeRefs must be an object");
  }
  const raw = value as Record<string, unknown>;
  const ids = raw.notebookIds;
  if (!Array.isArray(ids) || ids.some((id) => typeof id !== "string" || !id.trim())) {
    throw new TypeError("knowledgeRefs.notebookIds must be an array of non-empty strings");
  }
  if (raw.mode !== "fast" && raw.mode !== "detailed") {
    throw new TypeError('knowledgeRefs.mode must be "fast" or "detailed"');
  }
  const notebookIds = [...new Set((ids as string[]).map((id) => id.trim()))];
  if (notebookIds.length === 0) return null;
  return { notebookIds, mode: raw.mode };
}
