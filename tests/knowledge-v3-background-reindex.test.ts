import fs from "node:fs";
import crypto from "node:crypto";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnowledgeManager, type KnowledgeManagerOptions } from "./fixtures/knowledge-legacy/legacy-query-service.ts";
import { buildKnowledgeChunks, knowledgeBlockFingerprint, resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";
import { AnnIndexStore } from "../lib/knowledge/ann-index-store.ts";
import type { KnowledgeIngestionEmbedRequest } from "../lib/knowledge/ingestion-service.ts";

const homes: string[] = [], managers = new Set<KnowledgeManager>();
const studioId = "background-v3-studio", modelRef = { provider: "fake", id: "embedding-v3" };
const embed = vi.fn(async (request: KnowledgeIngestionEmbedRequest) => ({
  vectors: request.texts.map(text => [text.length + 1, 1, 2, 3]), dimensions: 4,
  model: { provider: modelRef.provider, id: modelRef.id, api: "openai", dimensions: 4 },
}));
const tick = () => new Promise<void>(resolve => setImmediate(resolve));
function open(home?: string, options: Partial<KnowledgeManagerOptions> = {}) {
  if (!home) { home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-v3-background-")); homes.push(home); }
  const manager = new KnowledgeManager({ lingxiHome: home, embedTextsForModel: embed, canEmbedWithModel: () => true,
    embeddingGate: { minRequestIntervalMs: 0 }, ingestionConcurrency: 1, ...options });
  managers.add(manager);
  return manager;
}
async function close(manager: KnowledgeManager) { managers.delete(manager); await manager.close(); }
afterEach(async () => {
  for (const manager of [...managers]) await close(manager);
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
  vi.restoreAllMocks(); embed.mockClear();
});
async function source(manager: KnowledgeManager, notebookId: string, text: string) {
  const imported = await manager.importPastedText({ studioId, notebookId, text, displayName: "迁移资料.txt" });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  return { sourceId: imported.source.id, artifactId: artifact.id, text };
}
async function legacyFixture(text = "旧索引里的苹果交付日期是九月十五日。") {
  const manager = open(), notebook = manager.createNotebook({ studioId, name: "旧版本资料" });
  manager.store.updateNotebookConfig({ studioId, notebookId: notebook.id, embeddingModelRef: modelRef });
  const item = await source(manager, notebook.id, text);
  const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: item.artifactId });
  // 真正调用保留的 v2 算法生成旧派生物，不能把 v3 结果改标签冒充旧索引。
  const config = resolveKnowledgeChunkerConfig(blocks, { targetChars: 1200, legacyVersion: "2" });
  const chunks = buildKnowledgeChunks(item.artifactId, blocks, { targetChars: 1200, legacyVersion: "2" });
  manager.store.db.prepare(`INSERT INTO chunk_profiles(id,profile_hash,strategy,target_chars,target_chars_source,
    chunker_version,structural_options_json,profile_type,created_at) VALUES(?,?,?,?,?,'2',NULL,'standard',?)`)
    .run(`cp_${config.configId}`, config.configId, config.strategy, 1200, "explicit", new Date().toISOString());
  const profile = manager.store.findOrCreateRetrievalProfile({ chunkProfileId: `cp_${config.configId}`,
    embeddingModelRef: modelRef, retrievalTopK: 12 });
  manager.store.db.prepare("UPDATE notebooks SET retrieval_profile_id=? WHERE id=?").run(profile.id, notebook.id);
  manager.indexStore.replaceArtifactChunks({ parseArtifactId: item.artifactId, chunkProfileHash: config.configId,
    blockFingerprint: knowledgeBlockFingerprint(blocks, { legacyVersion: "2" }), chunks });
  await manager.queryService.embedArtifactForIngestion({ runId: "legacy-v2-fixture", parseArtifactId: item.artifactId,
    chunkProfileHash: config.configId, embedTexts: request => embed({ ...request, modelRef }) });
  const oldVariant = manager.indexStore.resolveChunkIndexVariant(item.artifactId, config.configId)!;
  const oldVectors = manager.vectorIndex.listVariantsByChunkIndexVariant(oldVariant.id);
  const home = path.dirname(manager.knowledgeRoot);
  await close(manager);
  return { home, notebookId: notebook.id, ...item, oldVariant, oldVectors, chunks };
}

describe("v3 启动后低优先级重建", () => {
  it("启动同步阶段不建索引；分页扫描活跃最新来源，共享来源只登记一次，关闭取消未执行扫描", async () => {
    const first = open(), a = first.createNotebook({ studioId, name: "A" }), b = first.createNotebook({ studioId, name: "B" });
    const items = [];
    for (let index = 0; index < 22; index++) items.push(await source(first, a.id, `后台第${index}份独立资料。`));
    first.addSourceToNotebook({ studioId, notebookId: b.id, sourceId: items[0].sourceId });
    const removed = await source(first, a.id, "已经取消挂靠的资料。");
    first.store.removeSourceFromNotebook({ studioId, notebookId: a.id, sourceId: removed.sourceId });
    const deleted = await source(first, a.id, "已经删除的资料。");
    first.store.markSourceDeleted({ studioId, sourceId: deleted.sourceId });
    const removedNotebook = first.createNotebook({ studioId, name: "已经删除的笔记本" });
    await source(first, removedNotebook.id, "只属于已删除笔记本的资料。");
    first.store.deleteNotebook({ studioId, notebookId: removedNotebook.id });
    const latestText = "新快照还未解析，不可以用历史ready产物代替。";
    fs.writeFileSync(path.join(first.sourcesRoot, "latest.txt"), latestText);
    first.store.createContentSnapshot({ studioId, sourceId: items[0].sourceId,
      sha256: crypto.createHash("sha256").update(latestText).digest("hex"), mimeType: "text/plain",
      byteSize: Buffer.byteLength(latestText), storagePath: "sources/latest.txt" });
    const home = path.dirname(first.knowledgeRoot);
    await close(first);
    const manager = open(home);
    const queue = vi.spyOn(manager.ingestion, "enqueueBackgroundReindex");
    const index = vi.spyOn(manager.queryService, "indexArtifactForIngestion");
    expect(queue).not.toHaveBeenCalled(); expect(index).not.toHaveBeenCalled();
    expect(manager.store.listIngestionJobs({ studioId })).toHaveLength(0);
    await vi.waitFor(() => expect(queue).toHaveBeenCalledTimes(22));
    expect(new Set(queue.mock.calls.map(([input]) => input.sourceId)).size).toBe(22);
    expect(queue.mock.calls.find(([input]) => input.sourceId === items[0].sourceId)?.[0].parseArtifactId).toBeNull();
    expect(index).not.toHaveBeenCalled();
    expect(manager.store.listIngestionJobs({ studioId })).toHaveLength(0);
    await close(manager);
    const stopped = open(home), never = vi.spyOn(stopped.ingestion, "enqueueBackgroundReindex");
    await close(stopped); await tick();
    expect(never).not.toHaveBeenCalled();
  });

  it("查询缺少v2和v3时明确降级，真实入队推迟到查询返回后且并发重复请求合并", async () => {
    const manager = open(), notebook = manager.createNotebook({ studioId, name: "还没有索引" });
    const item = await source(manager, notebook.id, "没有索引的苹果日期资料。");
    const enqueue = vi.spyOn(manager.ingestion, "requestVariantBuild");
    const index = vi.spyOn(manager.queryService, "indexArtifactForIngestion");
    const query = { studioId, notebookIds: [notebook.id], question: "苹果日期", rerank: false };
    const results = await Promise.all([manager.queryService.retrieveForNotebooks(query), manager.queryService.retrieveForNotebooks(query)]);
    for (const result of results) {
      expect(result.candidates).toHaveLength(0);
      expect(result.degraded).toContainEqual(expect.objectContaining({ sourceId: item.sourceId, reason: "KNOWLEDGE_INDEX_MISSING" }));
    }
    expect(enqueue).not.toHaveBeenCalled(); expect(index).not.toHaveBeenCalled();
    await tick();
    expect(enqueue).toHaveBeenCalledTimes(1);
    expect(manager.store.listIngestionJobs({ studioId })).toHaveLength(1);
    expect(index).not.toHaveBeenCalled();
  });

  it("普通任务先执行；v3重建嵌入尚未结束时查询持续有结果，v2 FTS和旧向量逐行保留", async () => {
    const old = await legacyFixture();
    let release!: () => void, blocked = false;
    const wait = new Promise<void>(resolve => { release = resolve; });
    const manager = open(old.home, { embedTextsForModel: async request => {
      if (request.inputType !== "query" && request.texts.some(text => text.includes("苹果"))) { blocked = true; await wait; }
      return embed(request);
    } });
    await tick(); await tick();
    const foreground = await source(manager, old.notebookId, "优先处理普通入库的香蕉任务。");
    manager.enqueueSourceIngestion({ studioId, notebookId: old.notebookId, sourceId: foreground.sourceId, artifactId: foreground.artifactId });
    const parse = vi.spyOn(manager, "parseSource");
    const schedule = vi.spyOn(manager.vectorSearchBackend, "scheduleBuild");
    const oldRows = manager.indexStore.listVariantChunks(old.oldVariant.id);
    const drain = manager.ingestion.drainQueue();
    await vi.waitFor(() => expect(blocked).toBe(true));
    expect(parse.mock.calls.map(([input]) => input.sourceId)).toEqual([foreground.sourceId, old.sourceId]);
    const answer = await manager.queryService.retrieveForNotebooks({ studioId, notebookIds: [old.notebookId],
      question: "苹果交付日期", rerank: false });
    expect(answer.candidates.some(chunk => chunk.text.includes("苹果"))).toBe(true);
    expect(manager.indexStore.search({ scopes: [{ parseArtifactId: old.artifactId, chunkProfileHash: old.oldVariant.chunkProfileHash }],
      query: "苹果", limit: 5 }).length).toBeGreaterThan(0);
    release(); expect(await drain).toBe(2);
    expect(manager.indexStore.listVariantChunks(old.oldVariant.id)).toEqual(oldRows);
    expect(manager.vectorIndex.listVariantsByChunkIndexVariant(old.oldVariant.id)).toEqual(old.oldVectors);
    const variants = manager.indexStore.listChunkIndexVariantsByArtifact(old.artifactId);
    const current = variants.find(variant => variant.id !== old.oldVariant.id)!;
    expect(current.status).toBe("ready");
    expect(manager.store.getChunkProfile({ profileHash: current.chunkProfileHash }).chunkerVersion).toBe("3");
    const vector = manager.vectorIndex.listVariantsByChunkIndexVariant(current.id)[0];
    expect(vector.status).toBe("ready"); expect(schedule).toHaveBeenCalledWith(vector.id);
    const ann = new AnnIndexStore({ dbPath: path.join(manager.indexesRoot, "knowledge-ann.db") });
    try {
      await vi.waitFor(() => expect(ann.get(vector.id)?.status).toBe("ready"));
      expect(fs.statSync(path.join(manager.indexesRoot, "knowledge-ann", ann.get(vector.id)!.fileName)).size).toBeGreaterThan(0);
    } finally { ann.close(); }
  });

  it("旧v2待嵌入任务复用同一job退回分块，随后重启已ready来源不再入队", async () => {
    const old = await legacyFixture();
    const first = open(old.home);
    const shared = first.createNotebook({ studioId, name: "共享来源的另一个笔记本" });
    first.store.updateNotebookConfig({ studioId, notebookId: shared.id, embeddingModelRef: modelRef });
    first.addSourceToNotebook({ studioId, notebookId: shared.id, sourceId: old.sourceId });
    const chosen = first.store.listActiveLatestArtifactsForReindex()[0].notebookId;
    const jobNotebookId = [old.notebookId, shared.id].find(id => id !== chosen)!;
    const job = first.store.enqueueIngestionJob({ studioId, notebookId: jobNotebookId, sourceId: old.sourceId,
      artifactId: old.artifactId, chunkerConfigId: old.oldVariant.chunkProfileHash });
    first.store.claimIngestionJobById({ jobId: job.id });
    first.store.updateIngestionJobPhase({ studioId, jobId: job.id, phase: "embed" });
    first.store.markIngestionJobPendingEmbedding({ studioId, jobId: job.id });
    await close(first);
    const manager = open(old.home);
    await tick(); await tick();
    const rebuild = vi.spyOn(manager.queryService, "indexArtifactForIngestion");
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(rebuild).toHaveBeenCalled();
    expect(manager.store.listIngestionJobs({ studioId })).toHaveLength(1);
    expect(manager.store.getIngestionJob({ studioId, jobId: job.id })).toMatchObject({ status: "done", phase: "done" });
    await close(manager);
    const restarted = open(old.home), enqueue = vi.spyOn(restarted.ingestion, "enqueueBackgroundReindex");
    await tick(); await tick();
    expect(enqueue).not.toHaveBeenCalled();
    expect(await restarted.ingestion.drainQueue()).toBe(0);
  });

  it("低优先级嵌入在途时，新普通任务立即使用空闲worker，不等后台任务释放", async () => {
    const old = await legacyFixture();
    let release!: () => void, blocked = false;
    const wait = new Promise<void>(resolve => { release = resolve; });
    const manager = open(old.home, { ingestionConcurrency: 2, embedTextsForModel: async request => {
      if (request.texts.some(text => text.includes("苹果"))) { blocked = true; await wait; }
      return embed(request);
    } });
    await tick(); await tick();
    const drain = manager.ingestion.drainQueue();
    await vi.waitFor(() => expect(blocked).toBe(true));
    const foreground = await source(manager, old.notebookId, "在后台重建之后到达的香蕉普通任务。");
    const job = manager.enqueueSourceIngestion({ studioId, notebookId: old.notebookId,
      sourceId: foreground.sourceId, artifactId: foreground.artifactId });
    try {
      await vi.waitFor(() => expect(manager.store.getIngestionJob({ studioId, jobId: job.id }).status).toBe("done"));
      expect(manager.store.listIngestionJobs({ studioId, sourceId: old.sourceId })[0].status).toBe("running");
    } finally { release(); }
    expect(await drain).toBe(2);
  });

  it("嵌入硬窗口只在发送前校验，超过窗口明确失败且不远程请求，也不删除v2资料", async () => {
    const old = await legacyFixture("苹果项目完整原文".repeat(80));
    const manager = open(old.home, { getEmbeddingModelContextWindow: () => 32 });
    await tick(); await tick(); embed.mockClear();
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(embed).not.toHaveBeenCalled();
    expect(manager.store.listIngestionJobs({ studioId })[0]).toMatchObject({ status: "failed" });
    expect(manager.store.listIngestionJobs({ studioId })[0].error).toContain("KNOWLEDGE_INVALID_ARGUMENT");
    expect(manager.indexStore.listVariantChunks(old.oldVariant.id).map(chunk => chunk.text)).toEqual(old.chunks.map(chunk => chunk.text));
    expect(manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: old.notebookId })).toBe(2048);
  });
});
