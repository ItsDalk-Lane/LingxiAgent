import { expect, it } from "vitest";
import { searchVectorBackend } from "../lib/knowledge/vector-search-backend.ts";
import { KNOWLEDGE_RERANK_DISABLED_POLICY } from "../lib/knowledge/rerank-policy.ts";
import { annFixture } from "./helpers/knowledge-ann-fixture.ts";

it("确定性向量集 top-k overlap 至少 95%，来源 golden set 全部命中", async () => {
  const f = annFixture();
  try {
    let seed = 20260904;
    const random = () => { seed = Math.imul(seed, 1664525) + 1013904223 | 0; return (seed >>> 0) / 4294967296; };
    const sources = ["leave", "expense", "remote", "release", "oncall"];
    const vectors = sources.map(() => Array.from({ length: 240 }, () => [random() * 2 - 1, random() * 2 - 1, random() * 2 - 1]));
    const ids = sources.map((source, index) => f.add(source, vectors[index]));
    const backend = f.start(); await backend.whenIdle();
    let overlap = 0, expected = 0;
    for (let query = 0; query < 30; query++) {
      const input = { vectorIndexVariantIds: ids, model: f.model, queryVector: vectors[query % 5][query * 7], limit: 20 };
      const exact = f.portable.search(input), accelerated = await searchVectorBackend(backend, input);
      expect(accelerated.vectorBackend).toBe("hnsw"); expect(accelerated.degradedReasons).toEqual([]);
      const keys = new Set(exact.map(row => row.chunkId)); overlap += accelerated.results.filter(row => keys.has(row.chunkId)).length; expected += exact.length;
    }
    expect(overlap / expected).toBeGreaterThanOrEqual(0.95);
    for (const [index, source] of sources.entries()) {
      const results = await backend.search({ vectorIndexVariantIds: ids, model: f.model, queryVector: vectors[index][100], limit: 1 });
      expect(results[0].parseArtifactId).toBe(source);
    }
  } finally { await f.close(); }
});

it("真实知识入口的来源召回全部命中，HNSW 热查询只定点读块", async () => {
  const { KnowledgeManager } = await import("../lib/knowledge/knowledge-manager.ts");

  const { vi } = await import("vitest");
  const fs = await import("node:fs"), os = await import("node:os"), path = await import("node:path");
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-ann-golden-"));
  const topics = ["年假", "报销", "远程", "发布", "值班"];
  const manager = new KnowledgeManager({ lingxiHome: home, canEmbedWithModel: () => true,
    embedTextsForModel: async request => ({ dimensions: 5, model: { provider: "fake", id: "golden", api: "openai" },
      vectors: request.texts.map(text => topics.map(topic => text.includes(topic) ? 1 : 0)) }),
  });
  try {
    const notebook = manager.createNotebook({ studioId: "ann-golden", name: "制度本" });
    manager.updateNotebookSettings({ studioId: "ann-golden", notebookId: notebook.id, embeddingModelRef: { provider: "fake", id: "golden" } });
    const sourceIds: string[] = [];
    for (const topic of topics) {
      const imported = await manager.importPastedText({ studioId: "ann-golden", notebookId: notebook.id, displayName: `${topic}.txt`, text: `${topic} 制度的确定性说明。` });
      sourceIds.push(imported.source.id); manager.enqueueSourceIngestion({ studioId: "ann-golden", notebookId: notebook.id, sourceId: imported.source.id });
      await manager.ingestion.drainQueue();
    }
    const backend = manager.vectorSearchBackend as import("../lib/knowledge/usearch-vector-backend.ts").UseArchVectorBackend;
    await backend.whenIdle();
    const scope = manager.createTurnScope({ studioId: "ann-golden", sessionPath: "/tmp/ann-golden.jsonl", notebookIds: [notebook.id] });
    const compiledScope = await manager.compileTurnScope(scope);
    const fullRead = vi.spyOn(manager.indexStore, "listVariantChunks"); const blobRead = vi.spyOn(manager.vectorIndex, "readReadyVectorBatch");
    for (const [index, topic] of topics.entries()) {
      const result = await manager.searchService.searchWithEvidence({
        compiledScope,
        query: `${topic} 规定`,
        channel: "hybrid",
        limit: 1,
        rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY,
      });
      expect(result.response.vectorBackend).toBe("hnsw"); expect(result.response.degradedReasons).toEqual([]);
      expect(result.response.hits[0].sourceId).toBe(sourceIds[index]); expect(result.evidence.vectorBackend).toBe("hnsw");
    }
    expect(fullRead).not.toHaveBeenCalled(); expect(blobRead).not.toHaveBeenCalled();
  } finally { vi.restoreAllMocks(); await manager.close(); fs.rmSync(home, { recursive: true, force: true }); }
});
