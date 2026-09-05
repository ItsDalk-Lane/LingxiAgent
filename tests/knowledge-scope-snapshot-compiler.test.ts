import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { ScopeSnapshotCompiler } from "../lib/knowledge/scope-snapshot-compiler.ts";

const managers: KnowledgeManager[] = [];
const homes: string[] = [];
const studioId = "compiler-studio";
const sessionPath = "/tmp/compiler-session.jsonl";

afterEach(async () => {
  vi.restoreAllMocks();
  for (const manager of managers.splice(0)) await manager.close();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});

async function fixture(differentProfiles = false) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-scope-compiler-"));
  homes.push(home);
  const manager = new KnowledgeManager({ lingxiHome: home });
  managers.push(manager);
  const a = manager.createNotebook({ studioId, name: "甲" });
  const b = manager.createNotebook({ studioId, name: "乙" });
  if (differentProfiles) manager.updateNotebookSettings({ studioId, notebookId: b.id, chunkTargetChars: 200 });
  const imported = await manager.importPastedText({
    studioId, notebookId: a.id, displayName: "共享资料.txt", text: "知识检索需要冻结原始资料，保留精确引用。".repeat(50),
  });
  manager.addSourceToNotebook({ studioId, notebookId: b.id, sourceId: imported.source.id });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  for (const notebook of [a, b]) {
    manager.enqueueSourceIngestion({ studioId, notebookId: notebook.id, sourceId: imported.source.id, artifactId: artifact.id });
  }
  await manager.ingestion.drainQueue();
  const scope = manager.createTurnScope({ studioId, sessionPath, notebookIds: [a.id, b.id] });
  return { manager, a, b, imported, artifact, scope };
}

describe("冻结检索范围编译", () => {
  it("同一来源跨笔记本且配置相同，只保留一份来源和一个就绪变体", async () => {
    const { manager, scope } = await fixture();
    const result = await manager.compileTurnScope(scope);
    expect(result.sources).toHaveLength(1);
    expect(result.sources[0].notebookIds).toHaveLength(2);
    expect(result.sources[0].status).toBe("ready");
    expect(result.sources[0].chunkCount).toBeGreaterThan(0);
    expect(result.readyChunkVariantIds).toHaveLength(1);
    expect(result.notebooks.every(notebook => notebook.sourceIds.length === 1)).toBe(true);
  });

  it("同一来源不同配置保留不同变体，来源摘要仍唯一", async () => {
    const { manager, scope } = await fixture(true);
    const result = await manager.compileTurnScope(scope);
    expect(result.sources).toHaveLength(1);
    expect(result.readyChunkVariantIds).toHaveLength(2);
    expect(new Set(result.notebooks.map(notebook => notebook.chunkProfileHash)).size).toBe(2);
  });

  it("同一轮并发和缓存命中只编译一次，且不读取全文块或全部 chunk", async () => {
    const { manager, scope } = await fixture();
    const profileRead = vi.spyOn(manager.store, "getNotebookRetrievalProfileSnapshot");
    const metadataRead = vi.spyOn(manager.indexStore, "getReadyVariantMetadata");
    const blocks = vi.spyOn(manager.store, "listArtifactBlocks").mockImplementation(() => { throw new Error("全文读取被禁止"); });
    const chunks = vi.spyOn(manager.indexStore, "listVariantChunks").mockImplementation(() => { throw new Error("全量 chunk 读取被禁止"); });
    const calls = Array.from({ length: 5 }, () => manager.compileTurnScope(scope));
    expect(calls.every(call => call === calls[0])).toBe(true);
    const results = await Promise.all(calls);
    expect(await manager.compileTurnScope(scope)).toBe(results[0]);
    expect(profileRead).toHaveBeenCalledTimes(2);
    expect(metadataRead).toHaveBeenCalledTimes(1);
    expect(blocks).not.toHaveBeenCalled();
    expect(chunks).not.toHaveBeenCalled();
  });

  it("冻结版本不被后来的解析产物替换", async () => {
    const { manager, scope, imported, artifact } = await fixture();
    const newer = manager.store.beginParseArtifact({
      studioId, contentSnapshotId: imported.snapshot.id,
      parserId: "new-parser", parserVersion: "2", parserConfigHash: "a".repeat(64),
    });
    manager.store.completeParseArtifact({
      studioId, parseArtifactId: newer.id, status: "ready", warnings: [], semanticArtifactPath: `artifacts/${newer.id}.json`,
      blocks: [{ ordinal: 0, locatorType: "text", text: "新版本内容", locator: { lineNumber: 1 } }],
    });
    const result = await manager.compileTurnScope(scope);
    expect(result.sources[0].contentSnapshotId).toBe(imported.snapshot.id);
    expect(result.sources[0].parseArtifactId).toBe(artifact.id);
  });

  it("输入顺序和时间戳变化不会改变身份 hash，变体配置变化会改变 hash", async () => {
    const { manager, scope, b } = await fixture();
    const first = await manager.compileTurnScope(scope);
    manager.scopeCompiler.invalidateScope(scope.id);
    const reordered = { ...scope, createdAt: "2099-01-01", notebookIds: [...scope.notebookIds].reverse(),
      sources: [...scope.sources].reverse().map(source => ({ ...source, notebookIds: [...source.notebookIds].reverse() })) };
    const persisted = vi.spyOn(manager.store, "getTurnScope").mockReturnValue(reordered);
    expect((await manager.compileTurnScope(reordered)).snapshotHash).toBe(first.snapshotHash);
    persisted.mockRestore();
    manager.updateNotebookSettings({ studioId, notebookId: b.id, chunkTargetChars: 300 });
    await manager.ingestion.drainQueue();
    expect((await manager.compileTurnScope(scope)).snapshotHash).not.toBe(first.snapshotHash);
  });

  it("来源生命周期变化清缓存，移除成员关系不改变冻结来源清单", async () => {
    const { manager, scope, a, imported } = await fixture();
    const first = await manager.compileTurnScope(scope);
    manager.removeSourceFromNotebook({ studioId, notebookId: a.id, sourceId: imported.source.id });
    const second = await manager.compileTurnScope(scope);
    expect(second).not.toBe(first);
    expect(second.snapshotHash).toBe(first.snapshotHash);
    expect(second.sources[0].notebookIds).toContain(a.id);
  });

  it("关闭、被新轮替代和 manager 关闭后不能读取缓存", async () => {
    const { manager, scope, a } = await fixture();
    await manager.compileTurnScope(scope);
    manager.closeTurnScope({ scopeId: scope.id });
    await expect(manager.compileTurnScope(scope)).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    const second = manager.createTurnScope({ studioId, sessionPath, notebookIds: [a.id] });
    await manager.compileTurnScope(second);
    const third = manager.createTurnScope({ studioId, sessionPath, notebookIds: [a.id] });
    await expect(manager.compileTurnScope(second)).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    manager.close();
    managers.splice(managers.indexOf(manager), 1);
    await expect(manager.compileTurnScope(third)).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
  });

  it("部分来源尚未解析时保留可用来源，并显式标记缺口", async () => {
    const { manager, a, b } = await fixture();
    const pending = await manager.importPastedText({ studioId, notebookId: a.id, displayName: "待解析.txt", text: "尚未解析" });
    const scope = manager.createTurnScope({ studioId, sessionPath, notebookIds: [a.id, b.id] });
    const result = await manager.compileTurnScope(scope);
    expect(result.sources.find(source => source.sourceId === pending.source.id)?.status).toBe("parse_pending");
    expect(result.readyChunkVariantIds).toHaveLength(1);
    expect(result.warnings).toContain(`${pending.source.id}:parse_pending`);
  });

  it.each([
    ["parsing", "parse_pending"], ["needs_ocr", "needs_ocr"], ["failed", "index_failed"],
  ])("冻结产物处于 %s 时不使用既有索引并留下状态", async (status, expected) => {
    const { manager, scope, artifact, imported } = await fixture();
    manager.store.db.prepare("UPDATE parse_artifacts SET status = ? WHERE id = ?").run(status, artifact.id);
    const result = await manager.compileTurnScope(scope);
    expect(result.readyChunkVariantIds).toEqual([]);
    expect(result.sources[0].status).toBe(expected);
    expect(result.warnings).toContain(`${imported.source.id}:${expected}`);
  });

  it("编译排队后立即关闭 manager，不再触碰已关闭的数据库", async () => {
    const { manager, scope } = await fixture();
    const pending = manager.compileTurnScope(scope);
    manager.close();
    managers.splice(managers.indexOf(manager), 1);
    await expect(pending).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
  });

  it("未绑定配置时仅安排后台构建，编译过程不扫描原文", async () => {
    const { manager, scope } = await fixture();
    manager.store.db.prepare("UPDATE notebooks SET retrieval_profile_id = NULL").run();
    const blocks = vi.spyOn(manager.store, "listArtifactBlocks").mockImplementation(() => { throw new Error("不能现场扫描"); });
    const request = vi.spyOn(manager.ingestion, "requestVariantBuild");
    const result = await manager.compileTurnScope(scope);
    expect(result.readyChunkVariantIds).toEqual([]);
    expect(result.sources[0].status).toBe("index_missing");
    expect(request).not.toHaveBeenCalled();
    expect(blocks).not.toHaveBeenCalled();
    blocks.mockRestore();
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(request).toHaveBeenCalledTimes(2);
  });

  it("索引在建或失败时返回对应状态，全不可用也返回空集合", async () => {
    const { manager, scope } = await fixture();
    for (const [status, expected] of [["building", "index_building"], ["failed", "index_failed"]] as const) {
      manager.indexStore.db.prepare("UPDATE chunk_index_variants SET status = ?").run(status);
      manager.scopeCompiler.invalidateScope(scope.id);
      const result = await manager.compileTurnScope(scope);
      expect(result.sources[0].status).toBe(expected);
      expect(result.readyChunkVariantIds).toEqual([]);
    }
  });

  it("伪造来源列表不能扩大范围，跨工作室或会话不能命中缓存", async () => {
    const { manager, scope } = await fixture();
    const first = await manager.compileTurnScope(scope);
    expect((await manager.compileTurnScope({ ...scope, sources: [] })).sources).toEqual(first.sources);
    await expect(manager.compileTurnScope({ ...scope, studioId: "other" }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    await expect(manager.compileTurnScope({ ...scope, sessionPath: "/other" }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    const independent = new ScopeSnapshotCompiler({ store: manager.store, indexStore: manager.indexStore, requestVariantBuild: vi.fn() });
    await expect(independent.compile({ ...scope, studioId: "other" }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    independent.dispose();
  });
});
