import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { KnowledgeManager } from "../../lib/knowledge/knowledge-manager.ts";
import { createKnowledgeSearchTool } from "../../lib/tools/knowledge-search-tool.ts";
import type { KnowledgeToolSessionContext } from "../../lib/tools/knowledge-scope.ts";

export async function searchToolFixture(large = false) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-search-tool-"));
  const manager = new KnowledgeManager({ lingxiHome: home });
  const studioId = "search-tool", session = { sessionPath: "/tmp/knowledge-search-tool/main.jsonl", scopeOwnerSessionPath: "/tmp/knowledge-search-tool/main.jsonl" };
  try {
    const notebook = manager.createNotebook({ studioId, name: "资料本" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, chunkTargetChars: 2000 });
    const imported = await manager.importPastedText({ studioId, notebookId: notebook.id, displayName: "制度.txt",
      text: large ? Array.from({ length: 80 }, (_, index) => "needle ".repeat(220) + ` 条款 ${index}。`).join("\n\n") : "needle 年假规定：每年十五天。" });
    manager.enqueueSourceIngestion({ studioId, notebookId: notebook.id, sourceId: imported.source.id });
    await manager.ingestion.drainQueue();
    const scope = manager.createTurnScope({ studioId, sessionPath: session.sessionPath, notebookIds: [notebook.id] });
    const makeTool = (owner = studioId, context: KnowledgeToolSessionContext = session) => createKnowledgeSearchTool({
      getKnowledge: () => manager, getStudioId: () => owner, resolveSessionContext: () => context,
    });
    return { manager, studioId, session, notebook, source: imported.source, scope, makeTool,
      params: { scopeId: scope.id, query: "needle" },
      async close() { await manager.close(); fs.rmSync(home, { recursive: true, force: true }); },
    };
  } catch (error) { await manager.close(); fs.rmSync(home, { recursive: true, force: true }); throw error; }
}
