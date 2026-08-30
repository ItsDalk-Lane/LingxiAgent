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
 * Coverage run 未达 complete（任务书 §一百零四/§五十六，Phase 9）：expected 与
 * processed 不等、存在 failed/skipped primary units、或无可处理源（全
 * needs_ocr/unavailable）。终态留痕用，prompt 侧据此禁止"全文全部检查完成"措辞。
 */
export const KNOWLEDGE_COVERAGE_PARTIAL = "KNOWLEDGE_COVERAGE_PARTIAL";

/**
 * 单个 coverage shard 终态 failed（任务书 §一百零四/§五十二，Phase 9）：纠错
 * 重试一次 + shard 级 bounded retry（≤2 次自动重试）后仍无法产出合法 ShardResult。
 * 该 shard 的 primary units 计入 failed 分母，run 不得声称 complete。
 */
export const KNOWLEDGE_COVERAGE_SHARD_FAILED = "KNOWLEDGE_COVERAGE_SHARD_FAILED";

/**
 * Coverage run 被用户中止（任务书 §八十六/§一百零四，Phase 9 第二波）：执行中
 * abort signal 触发，pending/running shard 置 cancelled，completed 结果保留诊断。
 * 该轮 stats 覆盖状态记 cancelled，绝不生成 complete claim。
 */
export const KNOWLEDGE_COVERAGE_CANCELLED = "KNOWLEDGE_COVERAGE_CANCELLED";

/**
 * Coverage run 触发总时长上限（任务书 §一百零四，Phase 9 第二波）：到点后取消
 * 剩余 pending shard，run 以 partial 语义收尾（显式 timeout 留痕），不无限挂死会话。
 */
export const KNOWLEDGE_COVERAGE_TIMEOUT = "KNOWLEDGE_COVERAGE_TIMEOUT";
