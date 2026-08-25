export const MODEL_OPERATION_IDS = ["embedding", "rerank"] as const;

export type ModelOperation = (typeof MODEL_OPERATION_IDS)[number];

const MODEL_OPERATION_SET = new Set<string>(MODEL_OPERATION_IDS);

export function isModelOperation(value: unknown): value is ModelOperation {
  return typeof value === "string" && MODEL_OPERATION_SET.has(value);
}

/**
 * 操作能力和输入/输出模态是两条独立维度。旧目录里若用 type 标注
 * embedding/reranker，这里只负责兼容读取，不会把它写回成聊天模型。
 */
export function normalizeModelOperations(model: any): ModelOperation[] {
  const raw = Array.isArray(model?.operations) ? model.operations : [];
  const operations: ModelOperation[] = raw.filter(isModelOperation);
  if (model?.type === "embedding") operations.push("embedding");
  if (model?.type === "rerank" || model?.type === "reranker") operations.push("rerank");
  return [...new Set(operations)];
}

export function modelSupportsOperation(model: any, operation: ModelOperation): boolean {
  return normalizeModelOperations(model).includes(operation);
}
