import { KnowledgeQueryCache, normalizeKnowledgeQuery } from "./query-embedding-cache.ts";

/** 结果缓存身份包括冻结范围和全部过滤条件，防止跨轮复用越界。 */
export interface RetrievalResultCacheKey {
  scopeSnapshotHash: string;
  normalizedQuery: string;
  filters: { notebookIds?: string[]; sourceIds?: string[]; sectionKeys?: string[]; sectionsBySourceId?: Array<[string, string[]]> };
  limit: number;
  policyDigest: string;
  retrievalImplementationVersion: string;
}

export class RetrievalResultCache<T> {
  private readonly cache: KnowledgeQueryCache<T>;
  constructor(now?: () => number) { this.cache = new KnowledgeQueryCache(256, 2 * 60_000, now); }

  getOrCreate(key: RetrievalResultCacheKey, loader: (signal: AbortSignal) => Promise<T>, signal?: AbortSignal) {
    const ordered = (values?: string[]) => values === undefined ? null : [...new Set(values)].sort();
    return this.cache.getOrCreate(JSON.stringify([
      key.scopeSnapshotHash, normalizeKnowledgeQuery(key.normalizedQuery), key.policyDigest,
      ordered(key.filters.notebookIds), ordered(key.filters.sourceIds), ordered(key.filters.sectionKeys),
      key.filters.sectionsBySourceId?.map(([source, keys]) => [source, ordered(keys)]).sort(([a], [b]) => String(a).localeCompare(String(b))) ?? null,
      key.limit, key.retrievalImplementationVersion,
    ]), loader, signal);
  }

  clear(): void { this.cache.clear(); }
}
