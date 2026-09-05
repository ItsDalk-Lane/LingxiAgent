import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KNOWLEDGE_RERANK_DISABLED_POLICY } from "../lib/knowledge/rerank-policy.ts";
import { KnowledgeManager } from "./fixtures/knowledge-legacy/legacy-query-service.ts";
import { buildLegacyKnowledgeChunks, legacyKnowledgeBlockFingerprint, resolveLegacyKnowledgeChunkerConfig,
  KNOWLEDGE_CHUNK_TARGET_CHARS } from "../lib/knowledge/chunker.ts";

const managers: KnowledgeManager[] = [], homes: string[] = [];
const studioId = "fallback-studio";
afterEach(async () => {
  for (const manager of managers.splice(0)) await manager.close();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
  vi.restoreAllMocks();
});
async function fixture(auto = false) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-v2-fallback-")); homes.push(home);
  const manager = new KnowledgeManager({ lingxiHome: home }); managers.push(manager);
  const notebook = manager.createNotebook({ studioId, name: "旧资料" });
  if (!auto) manager.store.updateNotebookConfig({ studioId, notebookId: notebook.id, chunkTargetChars: 1200 });
  const sources = [];
  for (const name of ["苹果", "梨子"]) {
    const imported = await manager.importPastedText({ studioId, notebookId: notebook.id, displayName: `${name}.txt`,
      text: `${name}交付需要两位负责人批准。${name}交付日期为九月十五日。` });
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
    const targetChars = auto ? 6553 : 1200;
    const config = resolveLegacyKnowledgeChunkerConfig(blocks, { targetChars });
    manager.store.db.prepare(`INSERT OR IGNORE INTO chunk_profiles(id,profile_hash,strategy,target_chars,target_chars_source,
      chunker_version,structural_options_json,profile_type,created_at) VALUES(?,?,?,?,?,'2',NULL,'standard',?)`)
      .run(`cp_${config.configId}`, config.configId, config.strategy, targetChars, auto ? "auto" : "explicit", "2026-08-01T00:00:00Z");
    const profile = manager.store.findOrCreateRetrievalProfile({ chunkProfileId: `cp_${config.configId}` });
    manager.store.db.prepare("UPDATE notebooks SET retrieval_profile_id=? WHERE id=?").run(profile.id, notebook.id);
    const chunks = buildLegacyKnowledgeChunks(artifact.id, blocks, { targetChars });
    manager.indexStore.replaceArtifactChunks({ parseArtifactId: artifact.id, chunkProfileHash: config.configId,
      blockFingerprint: legacyKnowledgeBlockFingerprint(blocks), chunks });
    const variant = manager.indexStore.resolveChunkIndexVariant(artifact.id, config.configId)!;
    sources.push({ imported, artifact, blocks, config, chunks, variant });
  }
  const scope = manager.createTurnScope({ studioId, sessionPath: `/tmp/fallback-${crypto.randomUUID()}.jsonl`, notebookIds: [notebook.id] });
  return { manager, notebook, scope, sources };
}

describe("v3 重建期间继续使用真实 v2 索引", () => {
  it.each([false, true])("旧配置 auto=%s：编译只读旧就绪索引，快速答案与清单均指向实际 v2", async auto => {
    const f = await fixture(auto);
    const writes = vi.spyOn(f.manager.queryService, "indexArtifactForIngestion");
    const fullRead = vi.spyOn(f.manager.store, "listArtifactBlocks");
    const compiled = await f.manager.compileTurnScope(f.scope);
    expect(compiled.readyChunkVariantIds.sort()).toEqual(f.sources.map(source => source.variant.id).sort());
    expect(compiled.warnings.filter(warning => warning.includes("previous_chunk_version_rebuild_pending"))).toHaveLength(2);
    expect(writes).not.toHaveBeenCalled(); expect(fullRead).not.toHaveBeenCalled();
    const result = await f.manager.runFastKnowledgePipeline({ scope: f.scope, question: "苹果交付" });
    expect(result.stats.remoteModelCalls).toBe(0);
    expect(result.evidence.entries.length).toBeGreaterThan(0);
    expect(result.evidence.entries.every(entry => f.sources.some(source => source.variant.id === entry.chunkIndexVariantId
      && source.config.configId === entry.chunkProfileHash))).toBe(true);
    expect(writes).not.toHaveBeenCalled();
    const direct = await f.manager.queryService.retrieveForNotebooks({ studioId, notebookIds: [f.notebook.id], question: "苹果交付" });
    expect(direct.candidates.length).toBeGreaterThan(0);
    expect(direct.degraded.some(item => item.detail?.includes("serving ready v2"))).toBe(true);
  });

  it("一个来源已切 v3、另一个仍 v2，冻结集合与真实搜索/清单不漏来源也不串配置", async () => {
    const f = await fixture(true);
    const first = await f.manager.compileTurnScope(f.scope);
    const oldRows = f.manager.indexStore.db.prepare("SELECT * FROM knowledge_chunks ORDER BY id").all();
    const one = f.sources[0];
    f.manager.queryService.indexArtifactForIngestion(studioId, one.artifact.id, { targetChars: KNOWLEDGE_CHUNK_TARGET_CHARS });
    f.manager.store.resolveNotebookRetrievalProfile({ studioId, notebookId: f.notebook.id, strategy: one.config.strategy });
    // 同一份已冻结编译结果继续只读它登记的旧变体，不偷偷跳到新索引。
    const frozen = await f.manager.searchService.search({ compiledScope: first, channel: "fts",
      rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY, query: "交付", limit: 24 });
    expect(frozen.hits.every(hit => first.readyChunkVariantIds.includes(hit.chunkIndexVariantId))).toBe(true);
    f.manager.scopeCompiler.invalidateScope(f.scope.id);
    const next = await f.manager.compileTurnScope(f.scope);
    expect(next.snapshotHash).not.toBe(first.snapshotHash);
    expect(next.readyChunkVariantIds).toHaveLength(2);
    expect(next.readyChunkVariantIds).toContain(f.sources[1].variant.id);
    expect(next.readyChunkVariantIds).not.toContain(one.variant.id);
    const search = await f.manager.searchService.search({ compiledScope: next, channel: "fts",
      rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY, query: "交付", limit: 24 });
    expect(new Set(search.hits.map(hit => hit.sourceId))).toEqual(new Set(f.sources.map(source => source.imported.source.id)));
    const packed = await f.manager.runFastKnowledgePipeline({ scope: f.scope, question: "交付" });
    expect(new Set(packed.evidence.entries.map(entry => entry.sourceId))).toEqual(new Set(f.sources.map(source => source.imported.source.id)));
    for (const entry of packed.evidence.entries) expect(f.manager.indexStore.resolveChunkIndexVariant(entry.parseArtifactId, entry.chunkProfileHash)?.id)
      .toBe(entry.chunkIndexVariantId);
    const surviving = f.manager.indexStore.db.prepare("SELECT * FROM knowledge_chunks WHERE section_id IS NULL ORDER BY id").all();
    expect(surviving).toEqual(oldRows);
  });

  it("新旧索引都缺失时明确降级并在返回后入队，不能现场创建索引", async () => {
    const f = await fixture();
    f.manager.indexStore.reset();
    const build = vi.spyOn(f.manager.queryService, "indexArtifactForIngestion");
    const compiled = await f.manager.compileTurnScope(f.scope);
    expect(compiled.readyChunkVariantIds).toEqual([]);
    expect(compiled.sources.every(source => source.status === "index_missing")).toBe(true);
    expect(compiled.warnings.filter(warning => warning.endsWith(":index_missing"))).toHaveLength(2);
    expect(build).not.toHaveBeenCalled();
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(f.manager.store.listIngestionJobs({ studioId, notebookId: f.notebook.id, statuses: ["queued"] })).toHaveLength(2);
    expect(build).not.toHaveBeenCalled();
  });
});
