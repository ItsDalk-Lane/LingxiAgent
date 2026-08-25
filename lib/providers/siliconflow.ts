/**
 * SiliconFlow (硅基流动) provider plugin
 *
 * 聚合平台，支持 DeepSeek、Qwen、GLM、Llama 等 70+ 开源模型。
 * 文档：https://docs.siliconflow.cn
 */

/** @type {import('../../core/provider-registry.ts').ProviderPlugin} */
export const siliconflowPlugin = {
  id: "siliconflow",
  displayName: "SiliconFlow (硅基流动)",
  authType: "api-key",
  defaultBaseUrl: "https://api.siliconflow.cn/v1",
  defaultApi: "openai-completions",
  operationModels: [
    {
      id: "BAAI/bge-large-zh-v1.5",
      displayName: "BGE Large Zh v1.5",
      operations: ["embedding"],
      operationProtocol: "openai-embeddings",
      dimensions: 1024,
    },
    {
      id: "BAAI/bge-reranker-v2-m3",
      displayName: "BGE Reranker v2 M3",
      operations: ["rerank"],
      operationProtocol: "siliconflow-rerank",
    },
  ],
};
