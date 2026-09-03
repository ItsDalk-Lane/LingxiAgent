/** 查询嵌入缓存身份；实现与接线由紧随统一入口的缓存任务完成。 */
export interface QueryEmbeddingCacheKey {
  normalizedQuery: string;
  provider: string;
  modelId: string;
  modelConfigurationRevision: string;
  inputType: "query";
}
