/**
 * 知识库引用（knowledgeRefs）——聊天消息携带的笔记本级引用契约。
 *
 * 引用在会话内持续生效直到手动取消；前端每条消息显式携带（服务端无状态，
 * 与 sessionFileRefs 同模式）：
 *   wsMsg.knowledgeRefs = { notebookIds: string[], mode: "qa" | "assist" }
 *
 * Phase 7 只做透传 + 严格校验；Phase 8 的 knowledge-context-injector 在
 * core/desktop-session-submit.ts 的 prompt 组装区消费它做拆解与检索注入。
 */

import type { KnowledgeDegradeReason } from "./knowledge-reason-codes.ts";

export type KnowledgeReferenceMode = "qa" | "assist";

/**
 * Coverage 维度的两档枚举（任务书 §二十八 三维度正交，Phase 7）：
 * 与 answerMode（qa/assist）、retrievalMode（fts/hybrid）互不携带、互不影响。
 * 定义在 shared 层供 stats 契约与 lib/knowledge 的 planner 共用（lib → shared 单向）。
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
 * Source Fidelity 维度的等级枚举（任务书 §五十七/§五十九，Phase 9）：text
 * coverage 之外的第二个维度——「可解析文本全覆盖」绝不等于「原始资料全覆盖」。
 * 与 lib/knowledge/knowledge-coverage-manifest.ts 的 CoverageSourceFidelity 同构
 * （lib → shared 单向，这里只做 stats 摘要计数，不引 lib 类型）。
 */
export type KnowledgeCoverageFidelity =
  | "citation_grade"
  | "structural"
  | "semantic_only"
  | "needs_ocr"
  | "unavailable";

/** 各 fidelity 等级的源计数摘要（零计条目可省略）。 */
export type KnowledgeSourceFidelitySummary = Partial<Record<KnowledgeCoverageFidelity, number>>;

/**
 * EXHAUSTIVE 覆盖执行的终态（Phase 9 第二波）：
 * - complete：全部 primary units processed 且零 failed/skipped（gate 判定）；
 * - partial：存在 failed/未处理/超时截断（含 expected=0 的无可处理源）；
 * - cancelled：执行中被用户 abort。
 */
export type KnowledgeCoverageRunStatus = "complete" | "partial" | "cancelled";

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
  /** 证据总量超预算时走了分段压缩（知识提炼模型分批提炼后整合注入）。 */
  distilled?: boolean;
  /** 分段压缩的批数（成功路径）。 */
  distillBatches?: number;
  /** 压缩不可用/失败后退回"分片清单"降级的原因（显式留痕）。 */
  distillDegradedReason?: string;
  /**
   * Coverage 维度摘要（任务书 §二十八/§二十九，Phase 7）：本轮 KnowledgeCoveragePlan
   * 的结构化结论。只携带 plan 的判定结果，不携带任何 CoT/原始模型输出；缺省 =
   * 调用方（旧调用路径/降级块）未接入 coverage planner。
   */
  coverageMode?: KnowledgeCoverageMode;
  scopeLevel?: KnowledgeCoverageScopeLevel;
  /** exhaustive 档位才有完整性义务（coverageMode === "exhaustive" 时为 true）。 */
  requiresCompleteness?: boolean;
  /** 规则层命中的确定性规则 id（如 RULE_EXHAUSTIVE_KEYWORD / RULE_GLOBAL_NEGATIVE）。 */
  matchedRuleIds?: string[];
  /**
   * Phase 8/9 执行档位（§三十三~§三十七/§五十一）：本轮实际执行的覆盖档位。
   * plan 消费后与计划档位可能不同——high_recall 可被自动升级（见 upgradedTo）、
   * exhaustive 计划在 coverage 执行面不可用（worker 模型未配/无冻结 scope）时
   * 显式降级 broad 执行（见 coverageDegradeReason）。仅在接入 coverage planner
   * 时携带。
   */
  executedCoverageMode?: "high_recall" | "broad" | "exhaustive";
  /**
   * §四十一（执行侧）：high_recall 执行后 sourceCoverageFootprint 低于阈值且多源
   * scope，自动补一轮 broad 流程（复用已检索结果，只补缺失探测）时标注；
   * Phase 9 第二波起 broad 后 sectionCoverageFootprint 仍不足且整体性 scope 时
   * 也可自动升级 exhaustive（确定性全量扫描）。
   */
  upgradedTo?: "broad" | "exhaustive";
  /**
   * exhaustive 执行面不可用时的显式降格留痕（Phase 9 第二波；禁静默降级）：
   * 计划 exhaustive 但 worker 模型未配/冻结 scope 缺失/manifest 构建失败 →
   * 本轮按 broad 执行，原因记录在此并同步进注入块标注行。
   */
  coverageDegradeReason?: string;
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
  /**
   * ── EXHAUSTIVE 覆盖执行统计（任务书 §五十五~§五十七/§八十四~§八十六，
   * Phase 9 第二波；全部可选、向后兼容，仅 exhaustive 真执行时携带）──
   */
  /** coverage_runs.id（v14 持久化行；同 manifestHash 重开可续跑）。 */
  coverageRunId?: string;
  /**
   * 冻结覆盖清单指纹（coverage_runs.manifest_hash；任务书 §六十七 EvidenceManifest
   * 关联 exhaustive 轮用）。仅 exhaustive 真执行时携带。
   */
  coverageManifestHash?: string;
  /** 覆盖终态：complete 仅当 processed==expected>0 且 failed==skipped==0。 */
  coverageStatus?: KnowledgeCoverageRunStatus;
  coverageExpectedUnits?: number;
  coverageProcessedUnits?: number;
  coverageFailedUnits?: number;
  /** shard 终态计数（completed/failed；cancelled/pending 不进这两个分子）。 */
  coverageShardTotal?: number;
  coverageShardCompleted?: number;
  coverageShardFailed?: number;
  /** processed / expected（4 位小数；expected=0 时为 0，绝不虚标 100%）。 */
  textCoverageRatio?: number;
  /** 各 fidelity 等级的源计数（needs_ocr/unavailable 源在注入块 fidelity 行点名）。 */
  sourceFidelitySummary?: KnowledgeSourceFidelitySummary;
  /** aggregateShardEvidence 去重后的 findings 数（实际注入数见 injectedChunks）。 */
  coverageFindingsCount?: number;
  /** 终态留痕 code：KNOWLEDGE_COVERAGE_PARTIAL / _CANCELLED / _TIMEOUT。 */
  coverageReasonCode?: string;
  /**
   * Phase 10 层级归约统计（§六十一/§六十二；全部可选、向后兼容，仅 exhaustive
   * 真执行时携带）。levels 按层级聚合（source/notebook/cross_notebook）；
   * degradedReason 在归约降级（reduceModel 未配/两次输出非法/调用失败）时留痕，
   * 此时证据走结构化截断 + shard 清单（禁静默降级）。
   */
  coverageReduction?: {
    levels: Array<{
      level: "source" | "notebook" | "cross_notebook";
      inputCount: number;
      outputCount: number;
      reduced: boolean;
    }>;
    degradedReason?: string;
  };
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
  if (raw.mode !== "qa" && raw.mode !== "assist") {
    throw new TypeError('knowledgeRefs.mode must be "qa" or "assist"');
  }
  const notebookIds = [...new Set((ids as string[]).map((id) => id.trim()))];
  if (notebookIds.length === 0) return null;
  return { notebookIds, mode: raw.mode };
}
