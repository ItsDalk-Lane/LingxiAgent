import { KnowledgeManager } from "../../lib/knowledge/knowledge-manager.ts";

export const metadataStudio = "metadata-studio";

export async function createMetadataFixture(home: string) {
  const manager = new KnowledgeManager({ lingxiHome: home });
  const notebook = manager.createNotebook({ studioId: metadataStudio, name: "目录资料" });
  manager.updateNotebookSettings({ studioId: metadataStudio, notebookId: notebook.id, chunkTargetChars: 200 });
  const imported = await manager.importPastedText({
    studioId: metadataStudio, notebookId: notebook.id, displayName: "目录.txt",
    text: "开头。第一章知识目录。第二章后台补齐。",
  });
  const artifact = await manager.parseSource({ studioId: metadataStudio, sourceId: imported.source.id });
  manager.store.completeParseArtifact({
    studioId: metadataStudio, parseArtifactId: artifact.id, status: "ready", warnings: [],
    semanticArtifactPath: `artifacts/${artifact.id}.json`,
    blocks: [
      { ordinal: 0, locatorType: "text", text: "开头。", locator: { lineNumber: 1 } },
      { ordinal: 1, locatorType: "text", text: "第一章知识目录。", locator: { headingPath: ["第一章"], lineNumber: 2 } },
      { ordinal: 2, locatorType: "text", text: "第二章后台补齐。", locator: { headingPath: ["第二章"], lineNumber: 3 } },
    ],
  });
  manager.enqueueSourceIngestion({
    studioId: metadataStudio, notebookId: notebook.id, sourceId: imported.source.id, artifactId: artifact.id,
  });
  await manager.ingestion.drainQueue();
  const scope = manager.createTurnScope({
    studioId: metadataStudio, sessionPath: "/tmp/metadata-session.jsonl", notebookIds: [notebook.id],
  });
  const compiled = await manager.compileTurnScope(scope);
  const variant = manager.indexStore.resolveChunkIndexVariant(artifact.id, compiled.sources[0].chunkProfileHash!)!;
  manager.scopeCompiler.invalidateScope(scope.id);
  return { manager, notebook, imported, artifact, scope, variant };
}
