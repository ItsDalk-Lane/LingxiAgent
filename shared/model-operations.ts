export const MODEL_OPERATION_IDS = ["embedding", "rerank"] as const;

export type ModelOperation = (typeof MODEL_OPERATION_IDS)[number];

/**
 * 嵌入/重排模型 operationProtocol 的已识别协议清单（冻结）。
 * 客户端按 execution.api 匹配协议方言（URL/请求体/认证头/响应形状）；
 * 缺省或未识别的协议名一律回退 openai-embeddings / cohere 形状的现状路径。
 */
export const MODEL_OPERATION_PROTOCOLS = [
  "openai-embeddings",
  "ollama-embed",
  "gemini-embed",
  "voyage-embeddings",
  "cohere-rerank",
  "siliconflow-rerank",
  "voyage-rerank",
] as const;

export type ModelOperationProtocol = (typeof MODEL_OPERATION_PROTOCOLS)[number];

/**
 * 用户自添加模型只打操作标签（嵌入/重排），不指定协议时按供应商推断默认方言：
 * Ollama 用原生 embed 端点、Gemini 用原生 batchEmbedContents，其余（含 DashScope、
 * SiliconFlow 等所有 OpenAI 兼容网关）重排统一 cohere 形状、嵌入统一 openai 形状。
 * 未识别协议名在客户端会回退同一形状，因此该推断只是把回退显式化到目录条目上。
 */
export function inferOperationProtocol(providerId: string, operation: ModelOperation): ModelOperationProtocol {
  if (operation === "rerank") return "cohere-rerank";
  switch (providerId) {
    case "ollama":
      return "ollama-embed";
    case "gemini":
      return "gemini-embed";
    default:
      return "openai-embeddings";
  }
}

/**
 * rerank 单次请求的文档数硬上限（调用方裁剪与客户端校验共用）。
 * 查询侧与 RerankClient 曾分别写 200/100：候选落在 101–200 区间时重排输入被
 * 客户端断言拒绝，一次问答的检索整体失败（2026-08-29 事故）——单一真理源消除该类打架。
 */
export const MODEL_OPERATION_RERANK_MAX_DOCS = 100;

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
