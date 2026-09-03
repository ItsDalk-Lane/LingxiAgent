/** 结果缓存身份包括冻结范围和全部过滤条件，防止跨轮复用越界。 */
export interface RetrievalResultCacheKey {
  scopeSnapshotHash: string;
  normalizedQuery: string;
  channel: "fts" | "hybrid";
  filters: { notebookIds?: string[]; sourceIds?: string[]; sectionKeys?: string[] };
  limit: number;
  rerank: boolean;
  retrievalImplementationVersion: string;
}
