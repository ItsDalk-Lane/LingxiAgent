import { AsyncLocalStorage } from "node:async_hooks";
import type { VectorIndexModelIdentity, VectorSearchResult, VectorIndexAdapter } from "./vector-index-adapter.ts";

export interface KnowledgeVectorSearchBackend {
  readonly kind: "hnsw" | "portable";
  search(input: {
    vectorIndexVariantIds: string[];
    model: VectorIndexModelIdentity;
    queryVector: number[];
    limit: number;
  }): Promise<VectorSearchResult[]>;
  scheduleBuild(vectorIndexVariantId: string): void;
  invalidate(vectorIndexVariantId: string): void;
  close(): Promise<void>;
}

interface VectorSearchDiagnostics { vectorBackend: "hnsw" | "portable"; degradedReasons: string[] }
const diagnostics = new AsyncLocalStorage<VectorSearchDiagnostics>();

/** 每次查询各自记录降级，并发查询不能互相覆盖后端状态。 */
export function recordPortableVectorFallback(reason: string): void {
  const current = diagnostics.getStore();
  if (current) { current.vectorBackend = "portable"; current.degradedReasons.push(reason); }
}

export async function searchVectorBackend(backend: KnowledgeVectorSearchBackend, input: Parameters<KnowledgeVectorSearchBackend["search"]>[0]) {
  const report: VectorSearchDiagnostics = { vectorBackend: backend.kind, degradedReasons: [] };
  const results = await diagnostics.run(report, () => backend.search(input));
  return { results, ...report, degradedReasons: [...new Set(report.degradedReasons)] };
}

export class PortableKnowledgeVectorBackend implements KnowledgeVectorSearchBackend {
  readonly kind = "portable" as const;
  private readonly adapter: VectorIndexAdapter;
  private readonly reason: string | undefined;
  constructor(adapter: VectorIndexAdapter, reason?: string) { this.adapter = adapter; this.reason = reason; }
  async search(input: Parameters<KnowledgeVectorSearchBackend["search"]>[0]): Promise<VectorSearchResult[]> {
    if (this.reason) recordPortableVectorFallback(this.reason);
    return input.vectorIndexVariantIds.length ? this.adapter.search(input) : [];
  }
  scheduleBuild(_vectorIndexVariantId: string): void { /* 可移植数据由摄入流程维护。 */ }
  invalidate(_vectorIndexVariantId: string): void { /* 该后端没有派生内存索引。 */ }
  async close(): Promise<void> { /* 数据库由管理器持有，不能在回退层关闭。 */ }
}
