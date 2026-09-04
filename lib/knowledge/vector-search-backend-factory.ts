import fs from "node:fs";
import path from "node:path";
import { AnnIndexStore } from "./ann-index-store.ts";
import { UseArchVectorBackend, type KnowledgeNativeModule } from "./usearch-vector-backend.ts";
import { PortableKnowledgeVectorBackend, type KnowledgeVectorSearchBackend } from "./vector-search-backend.ts";
import type { PortableVectorIndexAdapter } from "./vector-index-adapter.ts";

/** 派生目录故障只能重建 ANN 自己的文件，不能删除已经付费得到的向量。 */
export function createKnowledgeVectorSearchBackend(options: {
  indexesRoot: string; portable: PortableVectorIndexAdapter; Database?: any;
  now?: () => string; log?: (message: string) => void; loadNative?: () => KnowledgeNativeModule;
}): KnowledgeVectorSearchBackend {
  const dbPath = path.join(options.indexesRoot, "knowledge-ann.db");
  let store: AnnIndexStore | undefined;
  try {
    try { store = new AnnIndexStore({ dbPath, Database: options.Database, now: options.now }); }
    catch {
      options.log?.("knowledge ANN: rebuilding unavailable derived catalog");
      for (const suffix of ["", "-wal", "-shm"]) fs.rmSync(`${dbPath}${suffix}`, { force: true });
      store = new AnnIndexStore({ dbPath, Database: options.Database, now: options.now });
    }
    // 原生扩展在后台构建和查询时加载；缺少可选依赖不影响应用启动。
    return new UseArchVectorBackend({ portable: options.portable, store,
      root: path.join(options.indexesRoot, "knowledge-ann"), loadNative: options.loadNative, log: options.log });
  } catch {
    store?.close();
    options.log?.("knowledge ANN: portable fallback (ANN_CATALOG_UNAVAILABLE)");
    return new PortableKnowledgeVectorBackend(options.portable, "ANN_CATALOG_UNAVAILABLE");
  }
}
