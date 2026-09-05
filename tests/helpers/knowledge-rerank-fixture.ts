import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { KnowledgeManager } from "../../lib/knowledge/knowledge-manager.ts";
import type { KnowledgeManagerOptions } from "../../lib/knowledge/knowledge-manager.ts";
import type { KnowledgeModelRef } from "../../lib/knowledge/types.ts";
import { KNOWLEDGE_RERANK_ENABLED_POLICY } from "../../lib/knowledge/rerank-policy.ts";

export async function createRerankFixture(refs: Array<KnowledgeModelRef | null>, rerank: KnowledgeManagerOptions["rerankForModel"], manyChunks = false) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-global-rerank-"));
  const manager = new KnowledgeManager({ lingxiHome: home, rerankForModel: rerank });
  const notebooks = [], sourceIds: string[] = [];
  try {
    for (const [index, ref] of refs.entries()) {
      const notebook = manager.createNotebook({ studioId: "rerank", name: `资料 ${index}` }); notebooks.push(notebook);
      manager.updateNotebookSettings({ studioId: "rerank", notebookId: notebook.id, chunkTargetChars: 200, rerankModelRef: ref });
      const imported = await manager.importPastedText({ studioId: "rerank", notebookId: notebook.id, displayName: `${index}.txt`,
        text: manyChunks ? Array.from({ length: 80 }, (_, i) => "needle ".repeat(240) + `章节 ${i}`).join("\n\n") : `needle 对应资料 ${index}。` });
      sourceIds.push(imported.source.id);
      manager.enqueueSourceIngestion({ studioId: "rerank", notebookId: notebook.id, sourceId: imported.source.id });
      await manager.ingestion.drainQueue();
    }
    const scope = manager.createTurnScope({ studioId: "rerank", sessionPath: "/tmp/global-rerank.jsonl", notebookIds: notebooks.map(item => item.id) });
    const compiledScope = await manager.compileTurnScope(scope);
    return { manager, notebooks, sourceIds,
      request: { compiledScope, query: "needle", channel: "hybrid" as const, limit: 60,
        rerankPolicy: KNOWLEDGE_RERANK_ENABLED_POLICY },
      close: async () => { await manager.close(); fs.rmSync(home, { recursive: true, force: true }); },
    };
  } catch (error) {
    await manager.close(); fs.rmSync(home, { recursive: true, force: true }); throw error;
  }
}
