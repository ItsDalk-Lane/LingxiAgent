/**
 * Knowledge 检索降级 reason codes（任务书 §一百零四 子集，Phase 2 Query/Index 分离）。
 *
 * 稳定枚举：查询侧降级留痕、注入块标注、knowledge_read 结果标注全部以此为准，
 * 禁止靠字符串 message 匹配判断降级状态。新增 code 只能追加，不改既有条目语义。
 */
export const KNOWLEDGE_DEGRADE_REASONS = [
  /** ChunkIndexVariant（chunk/FTS 索引变体）未建立：摄入尚未覆盖该 (artifact, profile)。 */
  "KNOWLEDGE_INDEX_MISSING",
  /** 索引变体存在但未就绪：building/retiring，或内容指纹过期（源已变更、重建在途）。 */
  "KNOWLEDGE_INDEX_BUILDING",
  /** 索引变体构建失败（显式终态 failed；重试走摄入手动 reingest，查询不自动重试）。 */
  "KNOWLEDGE_INDEX_FAILED",
  /** VectorIndexVariant 未就绪或查询嵌入不可用：本轮向量通道跳过，降级纯 FTS。 */
  "KNOWLEDGE_VECTOR_NOT_READY",
  /**
   * 查询嵌入请求失败（2026-08-30 延迟加固）：网络/HTTP/期限超时/响应非法——
   * 向量通道是检索增强层，失败降级纯 FTS 并显式留痕（不炸检索、不丢已算好的
   * FTS 候选），后台重建按既有 requestVariantBuild 幂等语义补跑。
   */
  "KNOWLEDGE_EMBEDDING_FAILED",
  /** 源解析产物 needs_ocr：无可检索文本，该源本轮不参与检索。 */
  "KNOWLEDGE_SOURCE_NEEDS_OCR",
] as const;

export type KnowledgeDegradeReason = (typeof KNOWLEDGE_DEGRADE_REASONS)[number];

export function isKnowledgeDegradeReason(value: unknown): value is KnowledgeDegradeReason {
  return typeof value === "string"
    && (KNOWLEDGE_DEGRADE_REASONS as readonly string[]).includes(value);
}

/**
 * 摄入 job 诊断 reason codes（任务书 §一百零四）：写入 ingestion_jobs.error 的
 * 稳定前缀（`CODE: message` 形态），禁止靠 message 字符串匹配判断中断状态。
 */
export const KNOWLEDGE_EMBEDDING_INTERRUPTED = "KNOWLEDGE_EMBEDDING_INTERRUPTED";

/**
 * KnowledgeTurnScope 权限违例（任务书 §一百零四，Phase 4）：knowledge_read 的
 * scopeId 缺失/伪造/跨 session/跨 studio/已关闭，或 sourceId/notebookId 超出
 * scope 冻结集合时，工具以该 code 显式拒绝（不回落到旧的全 studio 行为）。
 */
export const KNOWLEDGE_SCOPE_VIOLATION = "KNOWLEDGE_SCOPE_VIOLATION";

/**
 * @deprecated 以下五个 exhaustive 覆盖执行 reason code 随 exhaustive 档移除
 * （2026-08-31）退役：生产写入侧已无产出，定义保留一版仅供存量 stats/日志
 * 的读取兼容，不做语义变更。
 */
export const KNOWLEDGE_COVERAGE_PARTIAL = "KNOWLEDGE_COVERAGE_PARTIAL";
export const KNOWLEDGE_COVERAGE_SHARD_FAILED = "KNOWLEDGE_COVERAGE_SHARD_FAILED";
export const KNOWLEDGE_COVERAGE_CANCELLED = "KNOWLEDGE_COVERAGE_CANCELLED";
export const KNOWLEDGE_COVERAGE_TIMEOUT = "KNOWLEDGE_COVERAGE_TIMEOUT";
export const KNOWLEDGE_COVERAGE_CIRCUIT_BREAK = "KNOWLEDGE_COVERAGE_CIRCUIT_BREAK";
