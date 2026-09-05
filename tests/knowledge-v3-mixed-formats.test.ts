import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { buildLegacyKnowledgeChunks, legacyKnowledgeBlockFingerprint, resolveLegacyKnowledgeChunkerConfig,
  resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";
import type { KnowledgeIngestionEmbedRequest } from "../lib/knowledge/ingestion-service.ts";

const homes: string[] = [], managers: KnowledgeManager[] = [];
const studioId = "mixed-formats-studio", modelRef = { provider: "fixture", id: "mixed-embedding" };
afterEach(async () => {
  for (const manager of managers.splice(0)) await manager.close();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
  vi.restoreAllMocks();
});

async function fixture(legacyText = false) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-v3-mixed-")); homes.push(home);
  const embed = vi.fn(async (request: KnowledgeIngestionEmbedRequest) => ({
    vectors: request.texts.map(() => [1, 2, 3, 4]), dimensions: 4,
    model: { provider: modelRef.provider, id: modelRef.id, api: "openai", dimensions: 4 },
  }));
  const manager = new KnowledgeManager({ lingxiHome: home, embedTextsForModel: embed,
    canEmbedWithModel: () => true, embeddingGate: { minRequestIntervalMs: 0 } });
  managers.push(manager);
  const notebook = manager.createNotebook({ studioId, name: "混合格式" });
  manager.store.updateNotebookConfig({ studioId, notebookId: notebook.id, chunkTargetChars: 1200, embeddingModelRef: modelRef });
  const importRoot = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-v3-mixed-import-")); homes.push(importRoot);
  const markdownPath = path.join(importRoot, "markdown-source.md");
  fs.writeFileSync(markdownPath, "# 文档交付\n\nMarkdown 交付必须由两位负责人批准。\n");
  const imported = [
    await manager.importPastedText({ studioId, notebookId: notebook.id, displayName: "普通文本.txt",
      text: "文本交付必须在九月十五日前完成。" }),
    await manager.importFile({ studioId, notebookId: notebook.id, filePath: markdownPath }),
  ];
  const sources = [];
  for (const [ordinal, item] of imported.entries()) {
    const artifact = await manager.parseSource({ studioId, sourceId: item.source.id });
    const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
    let hash: string;
    if (ordinal === 0 && legacyText) {
      const config = resolveLegacyKnowledgeChunkerConfig(blocks, { targetChars: 1200 }); hash = config.configId;
      manager.store.db.prepare(`INSERT INTO chunk_profiles(id,profile_hash,strategy,target_chars,target_chars_source,
        chunker_version,structural_options_json,profile_type,created_at) VALUES(?,?,?,1200,'explicit','2',NULL,'standard',?)`)
        .run(`cp_${hash}`, hash, config.strategy, new Date().toISOString());
      manager.indexStore.replaceArtifactChunks({ parseArtifactId: artifact.id, chunkProfileHash: hash,
        blockFingerprint: legacyKnowledgeBlockFingerprint(blocks), chunks: buildLegacyKnowledgeChunks(artifact.id, blocks, { targetChars: 1200 }) });
    } else {
      const config = resolveKnowledgeChunkerConfig(blocks, { targetChars: 1200 }); hash = config.configId;
      manager.store.resolveNotebookRetrievalProfile({ studioId, notebookId: notebook.id, strategy: config.strategy });
      manager.queryService.indexArtifactForIngestion(studioId, artifact.id, { targetChars: 1200 });
    }
    await manager.queryService.embedArtifactForIngestion({ runId: `mixed-fixture-${ordinal}`, parseArtifactId: artifact.id,
      chunkProfileHash: hash, embedTexts: request => embed({ ...request, modelRef }) });
    const variant = manager.indexStore.resolveChunkIndexVariant(artifact.id, hash)!;
    sources.push({ sourceId: item.source.id, artifactId: artifact.id, hash, variant, blocks });
  }
  const snapshot = manager.store.getNotebookRetrievalProfileSnapshot({ studioId, notebookId: notebook.id });
  expect(manager.store.getChunkProfile({ profileHash: snapshot.chunkProfileHash! }).strategy).toBe("markdown");
  const scope = manager.createTurnScope({ studioId, notebookIds: [notebook.id],
    sessionPath: path.join(home, `${crypto.randomUUID()}.jsonl`) });
  embed.mockClear();
  return { manager, notebook, sources, scope, embed };
}

describe("同一笔记本的混合格式与新旧索引选择", () => {
  it("当前绑定最后一个Markdown策略时，两份v3来源都参与编译、FTS和快速证据清单", async () => {
    const f = await fixture();
    const fullRead = vi.spyOn(f.manager.store, "listArtifactBlocks");
    const compiled = await f.manager.compileTurnScope(f.scope);
    expect(compiled.readyChunkVariantIds.sort()).toEqual(f.sources.map(source => source.variant.id).sort());
    expect(compiled.sources.every(source => source.status === "ready")).toBe(true);
    expect(compiled.warnings.filter(warning => warning.includes("previous_chunk_version_rebuild_pending"))).toEqual([]);
    expect(fullRead).not.toHaveBeenCalled();
    const result = await f.manager.searchService.search({ compiledScope: compiled, query: "交付", channel: "fts", rerank: false, limit: 24 });
    expect(new Set(result.hits.map(hit => hit.sourceId))).toEqual(new Set(f.sources.map(source => source.sourceId)));
    expect(result.remoteModelCalls).toBe(0); expect(f.embed).not.toHaveBeenCalled();
    const packed = await f.manager.runFastKnowledgePipeline({ scope: f.scope, question: "交付" });
    expect(new Set(packed.evidence.entries.map(entry => entry.sourceId))).toEqual(new Set(f.sources.map(source => source.sourceId)));
    for (const entry of packed.evidence.entries) {
      const source = f.sources.find(source => source.sourceId === entry.sourceId)!;
      expect(entry).toMatchObject({ chunkIndexVariantId: source.variant.id, chunkProfileHash: source.hash });
    }
    expect(fullRead).not.toHaveBeenCalled();
  });

  it("混合格式共用一个查询嵌入组，每个向量与引用清单保留该来源的实际profile", async () => {
    const f = await fixture();
    const compiled = await f.manager.compileTurnScope(f.scope);
    const result = await f.manager.searchService.searchWithEvidence({ compiledScope: compiled, query: "交付", channel: "hybrid", rerank: false, limit: 24 });
    expect(new Set(result.response.hits.map(hit => hit.sourceId))).toEqual(new Set(f.sources.map(source => source.sourceId)));
    expect(result.response).toMatchObject({ retrievalMode: "hybrid", embeddingGroups: 1, remoteModelCalls: 1 });
    expect(f.embed).toHaveBeenCalledTimes(1);
    expect(result.evidence.searchedVectorVariants).toHaveLength(2);
    for (const source of f.sources) {
      expect(result.evidence.sources).toContainEqual(expect.objectContaining({ sourceId: source.sourceId,
        parseArtifactId: source.artifactId, chunkProfileHash: source.hash }));
      expect(result.evidence.searchedVectorVariants).toContainEqual(expect.objectContaining({
        parseArtifactId: source.artifactId, chunkIndexVariantId: source.variant.id, chunkProfileHash: source.hash }));
      expect(result.response.hits.some(hit => hit.sourceId === source.sourceId && hit.channels.includes("vector"))).toBe(true);
    }
  });

  it("旧v2纯文本与新v3Markdown并存；新索引完成后旧compiled集合不暗换，重编译才优先v3", async () => {
    const f = await fixture(true);
    const first = await f.manager.compileTurnScope(f.scope);
    expect(first.readyChunkVariantIds.sort()).toEqual(f.sources.map(source => source.variant.id).sort());
    expect(first.warnings.filter(warning => warning.includes("previous_chunk_version_rebuild_pending"))).toHaveLength(1);
    const oldTextRows = f.manager.indexStore.listVariantChunks(f.sources[0].variant.id);
    const before = await f.manager.runFastKnowledgePipeline({ scope: f.scope, question: "交付" });
    expect(new Set(before.evidence.entries.map(entry => entry.chunkProfileHash))).toEqual(new Set(f.sources.map(source => source.hash)));
    const current = f.manager.queryService.indexArtifactForIngestion(studioId, f.sources[0].artifactId, { targetChars: 1200 });
    f.manager.store.resolveNotebookRetrievalProfile({ studioId, notebookId: f.notebook.id, strategy: "fixed" });
    const frozen = await f.manager.searchService.search({ compiledScope: first, query: "交付必须", channel: "fts", rerank: false, limit: 24 });
    expect(new Set(frozen.hits.map(hit => hit.sourceId))).toEqual(new Set(f.sources.map(source => source.sourceId)));
    expect(frozen.hits.every(hit => first.readyChunkVariantIds.includes(hit.chunkIndexVariantId))).toBe(true);
    expect(frozen.hits.some(hit => hit.chunkIndexVariantId === current.chunkIndexVariantId)).toBe(false);
    f.manager.scopeCompiler.invalidateScope(f.scope.id);
    const next = await f.manager.compileTurnScope(f.scope);
    expect(next.snapshotHash).not.toBe(first.snapshotHash);
    expect(next.readyChunkVariantIds.sort()).toEqual([current.chunkIndexVariantId, f.sources[1].variant.id].sort());
    expect(next.warnings.filter(warning => warning.includes("previous_chunk_version_rebuild_pending"))).toEqual([]);
    const after = await f.manager.runFastKnowledgePipeline({ scope: f.scope, question: "交付" });
    expect(new Set(after.evidence.entries.map(entry => entry.chunkIndexVariantId))).toEqual(new Set(next.readyChunkVariantIds));
    expect(f.manager.indexStore.listVariantChunks(f.sources[0].variant.id)).toEqual(oldTextRows);
  });
});
