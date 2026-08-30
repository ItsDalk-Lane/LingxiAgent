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

export type KnowledgeReferenceMode = "qa" | "assist";

export interface KnowledgeRefs {
  notebookIds: string[];
  mode: KnowledgeReferenceMode;
}

/**
 * 一次知识库注入的检索统计（冻结契约）：随注入块一起产出，经
 * desktop-session-submit 持久化到 presentation 条目（knowledgeRetrieval 字段）
 * 并随 session_user_message / 历史合并透出，供前端渲染「检索 N 块 · 注入 M 块」。
 */
export interface KnowledgeRetrievalStats {
  mode: KnowledgeReferenceMode;
  retrievalMode: "hybrid" | "fts" | "none";
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
  }>;
  /** 证据总量超预算时走了分段压缩（知识提炼模型分批提炼后整合注入）。 */
  distilled?: boolean;
  /** 分段压缩的批数（成功路径）。 */
  distillBatches?: number;
  /** 压缩不可用/失败后退回"分片清单"降级的原因（显式留痕）。 */
  distillDegradedReason?: string;
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
