import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { buildCoverageUnits } from "../lib/knowledge/knowledge-coverage-unit.ts";
import { EvidenceReceiptService } from "../lib/knowledge/evidence-receipt-service.ts";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { ResearchStore } from "../lib/knowledge/research/research-store.ts";

const cleanup: Array<() => Promise<void>> = [];
afterEach(async () => { vi.restoreAllMocks(); for (const close of cleanup.splice(0).reverse()) await close(); });

async function setup(ready = true) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-completeness-lifecycle-"));
  const log = vi.fn<(message: string) => void>();
  const model = { provider: "fixture", id: "local-embedding", api: "openai" };
  const manager = new KnowledgeManager({ lingxiHome: path.join(root, "home"), orphanRetentionMs: 0,
    now: () => "2026-09-04T07:00:00.000Z", log, ingestionLog: log,
    canEmbedWithModel: () => true,
    embedTextsForModel: async ({ texts }) => ({ vectors: texts.map(() => [1, 0]), dimensions: 2, model }) });
  cleanup.push(async () => { await manager.close(); fs.rmSync(root, { recursive: true, force: true }); });
  const studioId = "completeness-lifecycle", notebook = manager.createNotebook({ studioId, name: "完整性资料" });
  const originalPath = path.join(root, "项目.txt");
  fs.writeFileSync(originalPath, "项目原始记录：交付日期为九月十五日，预算三十二万元。");
  const imported = await manager.importFile({ studioId, notebookId: notebook.id, filePath: originalPath });
  const sourceId = imported.source.id;
  const artifact = ready ? await manager.parseSource({ studioId, sourceId }) : null;
  const blocks = artifact ? manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id }) : [];
  if (artifact) {
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: model });
    await manager.ingestion.drainQueue();
  }
  const scope = manager.createTurnScope({ studioId, notebookIds: [notebook.id], sessionPath: path.join(root, "main.jsonl") });
  const research = new ResearchStore(manager.store);
  const run = research.createRun({ turnScopeId: scope.id, turnId: scope.turnId, parentSessionPath: scope.sessionPath,
    question: "所选资料有没有遗漏？", completenessPolicy: "scope_complete" });
  const files = () => {
    const found: Record<string, string> = {};
    const walk = (directory: string) => {
      if (!fs.existsSync(directory)) return;
      for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
        const fullPath = path.join(directory, entry.name);
        if (entry.isDirectory()) walk(fullPath); else found[path.relative(root, fullPath)] = fs.readFileSync(fullPath).toString("base64");
      }
    };
    walk(path.join(manager.sourcesRoot, sourceId)); walk(path.join(manager.artifactsRoot, imported.snapshot.id));
    return found;
  };
  const indexes = () => artifact ? manager.indexStore.listChunkIndexVariantsByArtifact(artifact.id).map(variant => ({
    variant, chunks: manager.indexStore.listVariantChunks(variant.id), vectors: manager.vectorIndex.listVariantsByChunkIndexVariant(variant.id),
  })) : [];
  const addCheck = (includeUnits: boolean) => {
    const checkId = "check-lifecycle";
    const units = includeUnits && artifact ? buildCoverageUnits({ sourceId, parseArtifactId: artifact.id, blocks }) : [];
    manager.store.db.prepare(`INSERT INTO knowledge_completeness_checks
      (id,research_run_id,policy,status,total_units,created_at,updated_at) VALUES (?,?,?,?,?,?,?)`)
      .run(checkId, run.id, "scope_complete", "pending", units.length, research.now(), research.now());
    for (const unit of units) manager.store.db.prepare(`INSERT INTO knowledge_completeness_units
      (check_id,coverage_unit_id,source_id,parse_artifact_id,block_id,start_offset,end_offset,status,updated_at)
      VALUES (?,?,?,?,?,?,?,?,?)`).run(checkId, unit.id, sourceId, unit.parseArtifactId, unit.blockId,
      unit.startOffset, unit.endOffset, "pending", research.now());
    return checkId;
  };
  const addReceipt = () => {
    if (!artifact) throw new Error("测试资料必须已解析");
    return new EvidenceReceiptService(research).issue({ runId: run.id, actorSessionId: null, sourceId,
      contentSnapshotId: imported.snapshot.id, parseArtifactId: artifact.id, blockId: blocks[0].id,
      startOffset: 0, endOffset: blocks[0].text.length, channel: "knowledge_read" });
  };
  const addReference = (kind: string) => {
    if (kind === "check-only" || kind === "units") { addCheck(kind === "units"); return; }
    const receipt = addReceipt();
    if (kind === "evidence-only") {
      const need = research.createNeed(run.id, { claim: "交付日期", kind: "fact", required: true,
        minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false });
      new EvidenceLedger(research).linkEvidence({ runId: run.id, needId: need.id, receiptId: receipt.id,
        quote: "交付日期为九月十五日", relation: "supports", rationale: "冻结原文明确记载" });
      // 模拟历史凭据缺失但已入账证据仍存在；不能因为凭据缺失就把证据引用忽略。
      manager.store.db.prepare("DELETE FROM knowledge_research_read_receipts WHERE id = ?").run(receipt.id);
    }
  };
  return { manager, studioId, notebook, sourceId, imported, artifact, scope, research, run, log, originalPath,
    files, indexes, addCheck, addReference };
}

describe("完整性调查引用的来源生命周期保护", () => {
  it.each(["receipt-only", "evidence-only", "check-only", "units"])("关闭范围后 %s 仍在删除标记前拒绝，任务、监听、索引和字节不动", async kind => {
    const f = await setup(); f.addReference(kind); f.manager.closeTurnScope({ scopeId: f.scope.id });
    const job = f.manager.enqueueSourceIngestion({ studioId: f.studioId, notebookId: f.notebook.id,
      sourceId: f.sourceId, artifactId: f.artifact!.id });
    expect(f.manager.store.countActiveTurnScopesForSource({ sourceId: f.sourceId })).toBe(0);
    expect(f.manager.store.countEvidenceManifestsForSource({ sourceId: f.sourceId })).toBe(0);
    expect(f.manager.store.hasResearchReferencesForSource({ sourceId: f.sourceId })).toBe(true);
    const beforeSource = f.manager.getSource({ studioId: f.studioId, sourceId: f.sourceId });
    const beforeFiles = f.files(), beforeIndexes = f.indexes(), beforeWatch = f.manager.watcher.getWatchStates();
    expect(Object.keys(beforeFiles).length).toBeGreaterThan(0);
    expect(beforeIndexes[0].chunks.length).toBeGreaterThan(0); expect(beforeIndexes[0].vectors.length).toBeGreaterThan(0);
    expect(beforeWatch.some(state => state.sourceId === f.sourceId)).toBe(true);
    const mark = vi.spyOn(f.manager.store, "markSourceDeleted"), untrack = vi.spyOn(f.manager.watcher, "untrackSource");
    const cancel = vi.spyOn(f.manager.ingestion, "cancelSourceJobs");
    await expect(f.manager.deleteSource({ studioId: f.studioId, sourceId: f.sourceId }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_CONFLICT", message: expect.stringContaining("research") });
    expect(mark).not.toHaveBeenCalled(); expect(untrack).not.toHaveBeenCalled(); expect(cancel).not.toHaveBeenCalled();
    expect(f.manager.getSource({ studioId: f.studioId, sourceId: f.sourceId })).toEqual(beforeSource);
    expect(f.manager.store.getIngestionJob({ studioId: f.studioId, jobId: job.id })).toEqual(job);
    expect(f.manager.watcher.getWatchStates()).toEqual(beforeWatch);
    expect(f.files()).toEqual(beforeFiles); expect(f.indexes()).toEqual(beforeIndexes);
    expect(f.manager.store.db.pragma("foreign_key_check")).toEqual([]);
  });

  it.each(["receipt-only", "evidence-only", "check-only", "units"])("孤儿清理与即时索引修剪保留 %s，明确记录研究引用原因", async kind => {
    const f = await setup(); f.addReference(kind); f.manager.closeTurnScope({ scopeId: f.scope.id });
    const beforeFiles = f.files(), beforeIndexes = f.indexes();
    f.manager.removeSourceFromNotebook({ studioId: f.studioId, notebookId: f.notebook.id, sourceId: f.sourceId });
    expect(f.log.mock.calls.some(([message]) => message.includes("orphan index prune skipped") && message.includes("research-referenced"))).toBe(true);
    expect(f.indexes()).toEqual(beforeIndexes);
    const report = f.manager.runOrphanSourceGc();
    expect(report.purged).toEqual([]);
    expect(report.skipped).toEqual([{ sourceId: f.sourceId, reason: "research-referenced" }]);
    expect(f.manager.getSource({ studioId: f.studioId, sourceId: f.sourceId }).deletedAt).toBeNull();
    expect(f.files()).toEqual(beforeFiles); expect(f.indexes()).toEqual(beforeIndexes);
  });

  it("只有检查关联的冻结范围也保护无解析产物的不可用来源，不伪造原文块", async () => {
    const f = await setup(false); f.addCheck(false); f.manager.closeTurnScope({ scopeId: f.scope.id });
    expect(f.scope.sources[0].parseArtifactId).toBeNull();
    expect(f.manager.store.db.prepare("SELECT COUNT(*) AS n FROM knowledge_completeness_units").get()).toEqual({ n: 0 });
    expect(f.manager.store.db.prepare("SELECT COUNT(*) AS n FROM knowledge_blocks").get()).toEqual({ n: 0 });
    const bytes = f.files();
    await expect(f.manager.deleteSource({ studioId: f.studioId, sourceId: f.sourceId })).rejects.toMatchObject({ code: "KNOWLEDGE_CONFLICT" });
    f.manager.removeSourceFromNotebook({ studioId: f.studioId, notebookId: f.notebook.id, sourceId: f.sourceId });
    expect(f.manager.runOrphanSourceGc().skipped).toEqual([{ sourceId: f.sourceId, reason: "research-referenced" }]);
    expect(f.files()).toEqual(bytes);
  });

  it("只有关闭的研究运行而无实际引用时，显式删除仍取消任务并清理派生索引与托管字节", async () => {
    const f = await setup(); f.manager.closeTurnScope({ scopeId: f.scope.id });
    expect(f.manager.store.hasResearchReferencesForSource({ sourceId: f.sourceId })).toBe(false);
    const job = f.manager.enqueueSourceIngestion({ studioId: f.studioId, notebookId: f.notebook.id,
      sourceId: f.sourceId, artifactId: f.artifact!.id });
    const result = await f.manager.deleteSource({ studioId: f.studioId, sourceId: f.sourceId });
    expect(result.cancelledJobs).toContain(job.id);
    expect(result.source.deletedAt).not.toBeNull();
    expect(f.manager.watcher.getWatchStates().some(state => state.sourceId === f.sourceId)).toBe(false);
    expect(f.indexes()).toEqual([]); expect(f.files()).toEqual({});
    expect(() => f.manager.getSource({ studioId: f.studioId, sourceId: f.sourceId })).toThrow();
    expect(fs.existsSync(f.originalPath)).toBe(true);
  });

  it("没有研究引用的孤儿仍按原流程清理，不因新增保护扩大保留范围", async () => {
    const f = await setup(); f.manager.closeTurnScope({ scopeId: f.scope.id });
    f.manager.removeSourceFromNotebook({ studioId: f.studioId, notebookId: f.notebook.id, sourceId: f.sourceId });
    expect(f.indexes().every(index => index.chunks.length === 0 && index.vectors.length === 0)).toBe(true);
    expect(f.manager.runOrphanSourceGc()).toMatchObject({ purged: [f.sourceId], skipped: [] });
    expect(f.files()).toEqual({});
  });
});
