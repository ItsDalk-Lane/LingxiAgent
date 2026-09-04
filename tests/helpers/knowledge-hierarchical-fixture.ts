import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { vi } from "vitest";
import { KnowledgeManager } from "../../lib/knowledge/knowledge-manager.ts";
import { resolveKnowledgeChunkerConfig } from "../../lib/knowledge/chunker.ts";
import type { KnowledgeIngestionEmbedRequest } from "../../lib/knowledge/ingestion-service.ts";

export const hierarchicalStudio = "hierarchical-studio";
export async function createHierarchicalFixture(documents: Array<{ name: string; sections: Array<{ heading: string; text: string }> }>, vectors = false) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-hierarchical-"));
  const modelRef = { provider: "fixture", id: "hierarchical-embedding" };
  const embed = vi.fn(async (request: KnowledgeIngestionEmbedRequest) => ({
    vectors: request.texts.map(text => text.includes("unrelated") ? [0, 1] : [1, 0]), dimensions: 2,
    model: { ...modelRef, api: "openai", dimensions: 2 },
  }));
  const manager = new KnowledgeManager({ lingxiHome: home, embedTextsForModel: embed,
    canEmbedWithModel: () => true, embeddingGate: { minRequestIntervalMs: 0 } });
  const notebook = manager.createNotebook({ studioId: hierarchicalStudio, name: "分层资料" });
  if (vectors) manager.store.updateNotebookConfig({ studioId: hierarchicalStudio, notebookId: notebook.id, embeddingModelRef: modelRef });
  const sources = [];
  for (const doc of documents) {
    const imported = await manager.importPastedText({ studioId: hierarchicalStudio, notebookId: notebook.id,
      displayName: doc.name, text: doc.sections.map(section => section.text).join("\n\n") });
    const artifact = await manager.parseSource({ studioId: hierarchicalStudio, sourceId: imported.source.id });
    manager.store.completeParseArtifact({ studioId: hierarchicalStudio, parseArtifactId: artifact.id,
      status: "ready", warnings: [], semanticArtifactPath: `artifacts/${artifact.id}.json`,
      blocks: doc.sections.map((section, ordinal) => ({ ordinal, locatorType: "text", text: section.text,
        locator: { headingPath: [section.heading], lineNumber: ordinal + 1 } })) });
    const blocks = manager.store.listArtifactBlocks({ studioId: hierarchicalStudio, parseArtifactId: artifact.id });
    const config = resolveKnowledgeChunkerConfig(blocks);
    const { chunkProfile } = manager.store.resolveNotebookRetrievalProfile({ studioId: hierarchicalStudio, notebookId: notebook.id, strategy: config.strategy });
    const indexed = manager.queryService.indexArtifactForIngestion(hierarchicalStudio, artifact.id);
    if (vectors) await manager.queryService.embedArtifactForIngestion({ runId: `fixture-${sources.length}`,
      parseArtifactId: artifact.id, chunkProfileHash: chunkProfile.profileHash,
      embedTexts: request => embed({ ...request, modelRef }) });
    sources.push({ imported, artifact, blocks, variantId: indexed.chunkIndexVariantId,
      sections: manager.indexStore.listArtifactSections(artifact.id), chunks: manager.indexStore.listVariantChunks(indexed.chunkIndexVariantId) });
  }
  const sessionPath = path.join(home, "owner.jsonl");
  const scope = manager.createTurnScope({ studioId: hierarchicalStudio, sessionPath, notebookIds: [notebook.id] });
  const compiledScope = await manager.compileTurnScope(scope);
  embed.mockClear();
  return { manager, notebook, sources, sessionPath, scope, compiledScope, embed,
    request: { compiledScope, query: "needle", channel: "hybrid" as const, rerank: false, limit: 24 },
    close() { manager.close(); fs.rmSync(home, { recursive: true, force: true }); } };
}
