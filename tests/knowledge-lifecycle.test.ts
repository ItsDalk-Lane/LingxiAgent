import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, describe, expect, it } from "vitest";

import { isKnowledgeError } from "../lib/knowledge/errors.ts";
import { resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";
import { knowledgeChunkIndexVariantId } from "../lib/knowledge/knowledge-index-store.ts";
import { KnowledgeManager, type KnowledgeManagerOptions } from "../lib/knowledge/knowledge-manager.ts";
import { KnowledgeStore } from "../lib/knowledge/knowledge-store.ts";
import { KNOWLEDGE_SCHEMA_VERSION } from "../lib/knowledge/knowledge-store.ts";
import { knowledgeVectorIndexVariantId } from "../lib/knowledge/vector-index-adapter.ts";
import type { KnowledgeEmbeddingGateLimits } from "../lib/knowledge/ingestion-service.ts";

const require = createRequire(import.meta.url);
const Database = require("better-sqlite3");

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];
const FAKE_MODEL_REF = { id: "emb-1", provider: "fake" };

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-lifecycle-"));
  tempDirs.push(dir);
  return dir;
}

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

async function waitFor(condition: () => boolean, timeoutMs = 4000): Promise<void> {
  const startedAt = Date.now();
  while (!condition()) {
    if (Date.now() - startedAt > timeoutMs) throw new Error("waitFor: condition not met in time");
    await sleep(10);
  }
}

interface EmbedTracker {
  active: number;
  maxActive: number;
  calls: number;
}

/** 延迟伪嵌入：向量按文本长度导出（8 维），延迟窗口内跨 job 并发可观测。 */
function createDelayedEmbedder(tracker: EmbedTracker, delayMs: number) {
  return async ({ texts }: { texts: string[] }) => {
    tracker.active += 1;
    tracker.calls += 1;
    tracker.maxActive = Math.max(tracker.maxActive, tracker.active);
    try {
      await sleep(delayMs);
      return {
        vectors: texts.map(text => {
          const vector = new Array(8).fill(0);
          vector[text.length % 8] = (text.length % 7) + 1;
          return vector;
        }),
        dimensions: 8,
        model: { provider: "fake", id: "emb-1", api: "openai", dimensions: 8 },
      } as any;
    } finally {
      tracker.active -= 1;
    }
  };
}

/** 受控伪嵌入：调用挂起直到测试显式 resolve，或 signal abort 立即拒绝（delete wins 用）。 */
function createControlledEmbedder() {
  const calls: Array<{ texts: string[]; aborted: boolean }> = [];
  const embedder = ({ texts, signal }: { texts: string[]; signal?: AbortSignal }) =>
    new Promise<any>((resolve, reject) => {
      const entry = { texts, aborted: false };
      calls.push(entry);
      signal?.addEventListener("abort", () => {
        entry.aborted = true;
        reject(Object.assign(new Error("aborted"), { name: "AbortError" }));
      });
      // 不主动 resolve：本 embedder 只用于验证 abort 取消路径。
      void resolve;
    });
  return {
    calls,
    embedder,
    lastRejected: () => calls[calls.length - 1]?.aborted === true,
  };
}

function createManager(
  lingxiHome: string,
  options: {
    embedder?: ((request: { texts: string[]; signal?: AbortSignal }) => Promise<any>) | null;
    concurrency?: number;
    embeddingGate?: KnowledgeEmbeddingGateLimits;
    orphanRetentionMs?: number;
    managerOverrides?: Partial<KnowledgeManagerOptions>;
  } = {},
) {
  const manager = new KnowledgeManager({
    lingxiHome,
    embedTextsForModel: (request) => {
      if (!options.embedder) return Promise.resolve(null);
      return options.embedder(request);
    },
    canEmbedWithModel: () => options.embedder != null,
    ingestionConcurrency: options.concurrency,
    embeddingGate: options.embeddingGate,
    orphanRetentionMs: options.orphanRetentionMs,
    ...options.managerOverrides,
  });
  managers.push(manager);
  return manager;
}

/** 与路由 POST sources 相同的调用序列：导入 → 解析 → 入队（返回 artifact 供断言）。 */
async function importTextSource(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  text: string,
  displayName = "源.txt",
) {
  const imported = await manager.importPastedText({ studioId, notebookId, text, displayName });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  const job = manager.enqueueSourceIngestion({
    studioId,
    notebookId,
    sourceId: imported.source.id,
    artifactId: artifact.id,
  });
  return { imported, artifact, job };
}

function chunkProfileHashOf(manager: KnowledgeManager, studioId: string, artifactId: string, targetChars: number) {
  const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifactId });
  return resolveKnowledgeChunkerConfig(blocks, { targetChars }).configId;
}

function countRows(manager: KnowledgeManager, sql: string, ...params: unknown[]): number {
  return Number((manager.store.db.prepare(sql).get(...params) as any)?.count ?? 0);
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe("Knowledge 生命周期：schema v12 迁移", () => {
  it("v11→v12：orphan/cancelled 列与活跃唯一索引就位；存量重复活跃行收敛为最新一条", () => {
    const dir = tempHome();
    const dbPath = path.join(dir, "k.db");
    const store = new KnowledgeStore({ dbPath, Database });
    const notebook = store.createNotebook({ studioId: "studio-a", name: "资料" });
    const imported = store.createSourceWithSnapshot({
      studioId: "studio-a",
      notebookId: notebook.id,
      sourceType: "pasted_text",
      displayName: "共享.txt",
      originMetadata: { kind: "pasted_text" },
      snapshot: {
        sha256: "a".repeat(64),
        mimeType: "text/plain",
        byteSize: 3,
        storagePath: "sources/src_x/snap_y.bin",
      },
    });
    // 回滚成 v11 形态：去掉 v12 全部新增物（先摘索引再删列）。
    store.db.exec(`
      DROP INDEX idx_ingestion_jobs_active;
      DROP INDEX idx_sources_orphaned;
      ALTER TABLE sources DROP COLUMN orphaned_at;
      ALTER TABLE ingestion_jobs DROP COLUMN cancelled_at;
    `);
    // 绕过 enqueue 去重直接造两条同 (notebook, source) 活跃 job（v11 库的真实脏数据形态）。
    store.db.exec(`
      INSERT INTO ingestion_jobs (
        id, notebook_id, source_id, phase, status, chunker_config_id, created_at, updated_at
      ) VALUES
        ('job_old', '${notebook.id}', '${imported.source.id}', 'parse', 'queued',
          'aaaaaaaaaaaaaaaa', '2026-08-01T00:00:00.000Z', '2026-08-01T00:00:00.000Z'),
        ('job_new', '${notebook.id}', '${imported.source.id}', 'parse', 'queued',
          'bbbbbbbbbbbbbbbb', '2026-08-02T00:00:00.000Z', '2026-08-02T00:00:00.000Z');
    `);
    store.db.pragma("user_version = 11");
    store.close();

    const migrated = new KnowledgeStore({ dbPath, Database });
    // v11 库沿迁移链升到当前版（v12 收敛活跃重复行 → v13 建覆盖计划表）。
    expect(migrated.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);
    // 重复活跃行收敛：保留最新（job_new 仍 queued），旧的显式 failed 留痕。
    const old = migrated.db.prepare(`SELECT status, error FROM ingestion_jobs WHERE id = 'job_old'`).get() as any;
    expect(old.status).toBe("failed");
    expect(String(old.error)).toContain("collapsed");
    const kept = migrated.db.prepare(`SELECT status FROM ingestion_jobs WHERE id = 'job_new'`).get() as any;
    expect(kept.status).toBe("queued");
    // DB 级活跃唯一：再直接插一条活跃重复行必须撞约束。
    expect(() => migrated.db.exec(`
      INSERT INTO ingestion_jobs (
        id, notebook_id, source_id, phase, status, chunker_config_id, created_at, updated_at
      ) VALUES ('job_dup', '${notebook.id}', '${imported.source.id}', 'parse', 'queued',
        'cccccccccccccccc', '2026-08-03T00:00:00.000Z', '2026-08-03T00:00:00.000Z');
    `)).toThrow();
    migrated.close();
  });

  it("应用层幂等 ensure 由唯一索引兜底：enqueue 重复目标只产生一个活跃 job", async () => {
    const manager = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    const { job } = await importTextSource(manager, studioId, notebook.id, "苹果项目的交付日期是九月十五日。");
    // §十七 幂等：重复 API 请求 / 重复 watcher 事件语义 → 同一 job，不建重复工作。
    const again = manager.enqueueSourceIngestion({
      studioId, notebookId: notebook.id, sourceId: job.sourceId, artifactId: job.artifactId,
    });
    expect(again.id).toBe(job.id);
    expect(countRows(
      manager,
      `SELECT COUNT(*) AS count FROM ingestion_jobs WHERE source_id = ? AND status IN ('queued','running','pending_embedding')`,
      job.sourceId,
    )).toBe(1);
  });
});

describe("Knowledge 生命周期：三层模型（§十八）", () => {
  it("移除一个 membership、其他笔记本仍引用 → Source 与派生索引全部保留", async () => {
    const manager = createManager(tempHome(), { embedder: createDelayedEmbedder({ active: 0, maxActive: 0, calls: 0 }, 0) });
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "A" });
    const notebookB = manager.createNotebook({ studioId, name: "B" });
    for (const notebook of [notebookA, notebookB]) {
      manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    }
    const { imported, artifact, job } = await importTextSource(
      manager, studioId, notebookA.id, "苹果项目的交付日期是九月十五日。\n火星项目的预算是八百万元。",
    );
    manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId: imported.source.id });
    manager.enqueueSourceIngestion({ studioId, notebookId: notebookB.id, sourceId: imported.source.id, artifactId: artifact.id });
    expect(await manager.ingestion.drainQueue()).toBe(2);

    manager.removeSourceFromNotebook({ studioId, notebookId: notebookA.id, sourceId: imported.source.id });

    const source = manager.getSource({ studioId, sourceId: imported.source.id });
    expect(source.orphanedAt).toBeNull();
    expect(source.deletedAt).toBeNull();
    // 快照/工件/blocks/索引全部保留，另一笔记本照常检索。
    expect(manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id }).length)
      .toBeGreaterThan(0);
    expect(manager.indexStore.listChunkIndexVariantsByArtifact(artifact.id).length).toBeGreaterThan(0);
    // GC 扫描不会把仍被引用的源纳入候选。
    const report = manager.runOrphanSourceGc();
    expect(report.purged).toEqual([]);
    expect(manager.getSource({ studioId, sourceId: imported.source.id }).id).toBe(imported.source.id);
    expect(manager.store.getIngestionJob({ studioId, jobId: job.id }).status).toBe("done");
  });

  it("移除最后 membership → orphan 标记；默认保留期内不物理删", async () => {
    const manager = createManager(tempHome(), { embedder: createDelayedEmbedder({ active: 0, maxActive: 0, calls: 0 }, 0) });
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    const { imported, artifact } = await importTextSource(manager, studioId, notebook.id, "第一段事实。");
    await manager.ingestion.drainQueue();

    manager.removeSourceFromNotebook({ studioId, notebookId: notebook.id, sourceId: imported.source.id });

    const orphan = manager.getSource({ studioId, sourceId: imported.source.id });
    expect(orphan.orphanedAt).not.toBeNull();
    // 默认 7 天保留期：GC 扫不到候选，一切物理痕迹保留。
    const report = manager.runOrphanSourceGc();
    expect(report.scanned).toBe(0);
    expect(report.purged).toEqual([]);
    expect(manager.store.db.prepare(`SELECT COUNT(*) AS count FROM sources WHERE id = ?`).get(imported.source.id))
      .toMatchObject({ count: 1 });
    expect(fs.existsSync(path.join(manager.sourcesRoot, imported.source.id))).toBe(true);
    expect(manager.indexStore.listChunkIndexVariantsByArtifact(artifact.id).length).toBeGreaterThan(0);

    // 复活：重新加入笔记本 → orphan 清除（§十八）。
    manager.addSourceToNotebook({ studioId, notebookId: notebook.id, sourceId: imported.source.id });
    expect(manager.getSource({ studioId, sourceId: imported.source.id }).orphanedAt).toBeNull();
  });

  it("过保留期 GC → 全部物理痕迹清空（行、字节、产物、FTS/向量变体）", async () => {
    const manager = createManager(tempHome(), {
      embedder: createDelayedEmbedder({ active: 0, maxActive: 0, calls: 0 }, 0),
      orphanRetentionMs: 1,
    });
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const { imported, artifact } = await importTextSource(manager, studioId, notebook.id, "第一段事实。\n第二段事实。");
    await manager.ingestion.drainQueue();
    const snapshotId = manager.store.getLatestContentSnapshotForSource({
      studioId, sourceId: imported.source.id,
    }).id;
    const profileHash = chunkProfileHashOf(manager, studioId, artifact.id, 6553);
    const civId = knowledgeChunkIndexVariantId(artifact.id, profileHash);
    expect(manager.indexStore.listChunkIndexVariantsByArtifact(artifact.id).length).toBeGreaterThan(0);
    expect(manager.vectorIndex.listVariantsByChunkIndexVariant(civId).length).toBeGreaterThan(0);

    manager.removeSourceFromNotebook({ studioId, notebookId: notebook.id, sourceId: imported.source.id });
    await sleep(5); // 过保留期（retention=1ms）。

    const report = manager.runOrphanSourceGc();
    expect(report.purged).toEqual([imported.source.id]);
    expect(report.skipped).toEqual([]);
    // 事实行全部清空（逐表显式校验，含跨表引用链）。
    expect(countRows(manager, `SELECT COUNT(*) AS count FROM sources WHERE id = ?`, imported.source.id)).toBe(0);
    expect(countRows(manager, `SELECT COUNT(*) AS count FROM content_snapshots WHERE id = ?`, snapshotId)).toBe(0);
    expect(countRows(manager, `SELECT COUNT(*) AS count FROM parse_artifacts WHERE id = ?`, artifact.id)).toBe(0);
    expect(countRows(manager, `SELECT COUNT(*) AS count FROM knowledge_blocks WHERE parse_artifact_id = ?`, artifact.id)).toBe(0);
    expect(countRows(manager, `SELECT COUNT(*) AS count FROM ingestion_jobs WHERE source_id = ?`, imported.source.id)).toBe(0);
    expect(countRows(manager, `SELECT COUNT(*) AS count FROM notebook_sources WHERE source_id = ?`, imported.source.id)).toBe(0);
    // 托管字节与解析产物文件删除。
    expect(fs.existsSync(path.join(manager.sourcesRoot, imported.source.id))).toBe(false);
    expect(fs.existsSync(path.join(manager.artifactsRoot, snapshotId))).toBe(false);
    // 派生索引（FTS chunk 变体 / 向量变体）删除。
    expect(manager.indexStore.listChunkIndexVariantsByArtifact(artifact.id)).toEqual([]);
    expect(manager.vectorIndex.listVariantsByChunkIndexVariant(civId)).toEqual([]);
    // 删除后读取显式 404。
    expect(() => manager.getSource({ studioId, sourceId: imported.source.id })).toThrow();
  });

  it("有活跃 turn scope 冻结引用 → GC 跳过；scope 关闭后可清", async () => {
    const manager = createManager(tempHome(), {
      embedder: createDelayedEmbedder({ active: 0, maxActive: 0, calls: 0 }, 0),
      orphanRetentionMs: 1,
    });
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const { imported } = await importTextSource(manager, studioId, notebook.id, "冻结事实。");
    await manager.ingestion.drainQueue();
    // 本轮 scope 冻结该源（membership 仍活跃时创建）。
    const scope = manager.createTurnScope({
      studioId, sessionPath: "/sessions/s1.jsonl", turnId: "turn-1", notebookIds: [notebook.id],
    });
    expect(scope.sources.some(source => source.sourceId === imported.source.id)).toBe(true);

    manager.removeSourceFromNotebook({ studioId, notebookId: notebook.id, sourceId: imported.source.id });
    await sleep(5);
    const skipped = manager.runOrphanSourceGc();
    expect(skipped.purged).toEqual([]);
    expect(skipped.skipped).toEqual([
      { sourceId: imported.source.id, reason: "active-turn-scope-reference" },
    ]);
    expect(manager.store.db.prepare(`SELECT COUNT(*) AS count FROM sources WHERE id = ?`).get(imported.source.id))
      .toMatchObject({ count: 1 });

    manager.closeTurnScope({ scopeId: scope.id });
    const purged = manager.runOrphanSourceGc();
    expect(purged.purged).toEqual([imported.source.id]);
  });

  it("GC 防御：orphan 标记与活跃 membership 并存（复活窗口）→ 清标记跳过不误删", async () => {
    const manager = createManager(tempHome(), { orphanRetentionMs: 0 });
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "A" });
    const notebookB = manager.createNotebook({ studioId, name: "B" });
    const { imported } = await importTextSource(manager, studioId, notebookA.id, "共享事实。");
    manager.removeSourceFromNotebook({ studioId, notebookId: notebookA.id, sourceId: imported.source.id });
    expect(manager.getSource({ studioId, sourceId: imported.source.id }).orphanedAt).not.toBeNull();
    // 并发窗口模拟：membership 已复活但 orphan 标记漏清（直接造脏数据，防御路径）。
    manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId: imported.source.id });
    manager.store.markSourceOrphaned({ studioId, sourceId: imported.source.id });

    const report = manager.runOrphanSourceGc();
    expect(report.purged).toEqual([]);
    expect(report.skipped).toEqual([
      { sourceId: imported.source.id, reason: "active-membership-reappeared" },
    ]);
    expect(manager.getSource({ studioId, sourceId: imported.source.id }).orphanedAt).toBeNull();
  });
});

describe("Knowledge 生命周期：deleteSource（§十九 delete wins）", () => {
  it("queued job 直接取消；运行中 reindex 被 abort；一切不复活", async () => {
    const controlled = createControlledEmbedder();
    const manager = createManager(tempHome(), {
      embedder: controlled.embedder,
      concurrency: 1,
      embeddingGate: { maxConcurrent: 1, minRequestIntervalMs: 0 },
    });
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "A" });
    const notebookB = manager.createNotebook({ studioId, name: "B" });
    for (const notebook of [notebookA, notebookB]) {
      manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    }
    const text = "苹果项目的交付日期是九月十五日。\n火星项目的预算是八百万元。";
    const { imported, artifact, job } = await importTextSource(manager, studioId, notebookA.id, text);
    manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId: imported.source.id });
    manager.enqueueSourceIngestion({ studioId, notebookId: notebookB.id, sourceId: imported.source.id, artifactId: artifact.id });

    // 后台排空：notebookA 的 job 进入 embed 相位并挂在受控嵌入上（模拟运行中 reindex）。
    const drained = manager.ingestion.drainQueue();
    await waitFor(() => controlled.calls.length === 1
      && manager.store.getIngestionJob({ studioId, jobId: job.id }).phase === "embed");
    expect(manager.store.getIngestionJob({ studioId, jobId: job.id }).status).toBe("running");

    const result = await manager.deleteSource({ studioId, sourceId: imported.source.id });
    await drained;

    // 两条活跃 job（一条 running、一条 queued）全部取消留痕。
    expect(result.cancelledJobs).toEqual(expect.arrayContaining([job.id]));
    expect(result.cancelledJobs).toHaveLength(2);
    // 运行中的嵌入调用被 abort（不是跑完）。
    expect(controlled.lastRejected()).toBe(true);
    // 事实行与派生物理清理。
    expect(countRows(manager, `SELECT COUNT(*) AS count FROM sources WHERE id = ?`, imported.source.id)).toBe(0);
    expect(countRows(manager, `SELECT COUNT(*) AS count FROM ingestion_jobs WHERE source_id = ?`, imported.source.id)).toBe(0);
    expect(manager.indexStore.listChunkIndexVariantsByArtifact(artifact.id)).toEqual([]);
    // delete wins：reingest / 再入队 / 查询侧补齐全部显式失败，不复活。
    expect(() => manager.requeueSourceIngestion({ studioId, notebookId: notebookA.id, sourceId: imported.source.id }))
      .toThrow(/not found/i);
    expect(() => manager.enqueueSourceIngestion({ studioId, notebookId: notebookA.id, sourceId: imported.source.id }))
      .toThrow();
    let revived = false;
    try {
      manager.requestVariantBuild({ studioId, notebookId: notebookA.id, sourceId: imported.source.id, artifactId: artifact.id });
      revived = true;
    } catch (error) {
      expect(isKnowledgeError(error)).toBe(true);
    }
    expect(revived).toBe(false);
    // 二次删除显式 404。
    await expect(manager.deleteSource({ studioId, sourceId: imported.source.id })).rejects.toThrow();
  });

  it("cancelled job 拒绝手动重试（failed+cancelled_at 不可 requeue，delete wins 不复活）", async () => {
    const manager = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    const { imported, job } = await importTextSource(manager, studioId, notebook.id, "排队事实。");
    // 单独验证取消语义（不经 deleteSource 的 purge，job 行保留可查）。
    const cancelledIds = manager.store.cancelSourceIngestionJobs({
      sourceId: imported.source.id,
      reason: "KNOWLEDGE_SOURCE_DELETED: test",
    });
    expect(cancelledIds).toEqual([job.id]);
    const cancelled = manager.store.getIngestionJob({ studioId, jobId: job.id });
    expect(cancelled.status).toBe("failed");
    expect(cancelled.cancelledAt).not.toBeNull();
    expect(String(cancelled.error)).toContain("KNOWLEDGE_SOURCE_DELETED");
    // delete wins：cancelled job 永久不可手动重试。
    try {
      manager.store.requeueIngestionJob({ studioId, jobId: job.id });
      expect.unreachable("requeue should reject cancelled jobs");
    } catch (error) {
      expect(isKnowledgeError(error)).toBe(true);
      expect((error as any).code).toBe("KNOWLEDGE_CONFLICT");
    }
  });

  it("活跃 turn scope 冻结引用 → deleteSource 显式 409 拒绝", async () => {
    const manager = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    const { imported } = await importTextSource(manager, studioId, notebook.id, "冻结事实。");
    manager.createTurnScope({ studioId, sessionPath: "/sessions/s2.jsonl", turnId: "t1", notebookIds: [notebook.id] });
    await expect(manager.deleteSource({ studioId, sourceId: imported.source.id }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_CONFLICT" });
    // 拒绝路径不产生任何清理。
    expect(manager.getSource({ studioId, sourceId: imported.source.id }).deletedAt).toBeNull();
  });
});

describe("Knowledge 摄入并发模型（§十六）", () => {
  it("两个不同 (artifact, profile) 的 job 并行（嵌入调用交错）", async () => {
    const tracker: EmbedTracker = { active: 0, maxActive: 0, calls: 0 };
    const manager = createManager(tempHome(), {
      embedder: createDelayedEmbedder(tracker, 60),
      concurrency: 2,
      embeddingGate: { maxConcurrent: 2, minRequestIntervalMs: 0 },
    });
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    await importTextSource(manager, studioId, notebook.id, "苹果项目的交付日期是九月十五日。");
    await importTextSource(manager, studioId, notebook.id, "火星项目的预算是八百万元。");
    expect(await manager.ingestion.drainQueue()).toBe(2);
    // 两个不同 civ 的 job 在 embed 相位真正交错（并行的是异步嵌入 IO）。
    expect(tracker.maxActive).toBe(2);
  });

  it("同 key（同 artifact+profile）两个 job 串行：嵌入调用不重叠", async () => {
    const tracker: EmbedTracker = { active: 0, maxActive: 0, calls: 0 };
    const manager = createManager(tempHome(), {
      embedder: createDelayedEmbedder(tracker, 60),
      concurrency: 2,
      embeddingGate: { maxConcurrent: 2, minRequestIntervalMs: 0 },
    });
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "A" });
    const notebookB = manager.createNotebook({ studioId, name: "B" });
    for (const notebook of [notebookA, notebookB]) {
      manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    }
    const { imported, artifact } = await importTextSource(
      manager, studioId, notebookA.id, "苹果项目的交付日期是九月十五日。\n火星项目的预算是八百万元。",
    );
    manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId: imported.source.id });
    manager.enqueueSourceIngestion({ studioId, notebookId: notebookB.id, sourceId: imported.source.id, artifactId: artifact.id });
    expect(await manager.ingestion.drainQueue()).toBe(2);
    // 同一 ChunkIndexVariant 的两个 job（同 artifact、同 profile）被 key 锁串行。
    expect(tracker.maxActive).toBe(1);
    expect(tracker.calls).toBe(2); // 第二个 job 只发探测批（hasArtifact 命中即跳过）。
  });

  it("Provider Semaphore 限流：同 (provider, model) 并发嵌入请求数不超上限", async () => {
    const tracker: EmbedTracker = { active: 0, maxActive: 0, calls: 0 };
    const manager = createManager(tempHome(), {
      embedder: createDelayedEmbedder(tracker, 40),
      concurrency: 3,
      embeddingGate: { maxConcurrent: 1, minRequestIntervalMs: 0 },
    });
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    await importTextSource(manager, studioId, notebook.id, "事实一。");
    await importTextSource(manager, studioId, notebook.id, "事实二。");
    await importTextSource(manager, studioId, notebook.id, "事实三。");
    expect(await manager.ingestion.drainQueue()).toBe(3);
    expect(tracker.calls).toBe(3);
    // 3 个无 key 冲突的 job 本可全并行，但同 provider 信号量限到并发 1。
    expect(tracker.maxActive).toBe(1);
  });

  it("最小请求间隔生效：同 provider 两次派发至少间隔 minRequestIntervalMs", async () => {
    const tracker: EmbedTracker = { active: 0, maxActive: 0, calls: 0 };
    const dispatchTimes: number[] = [];
    const embedder = async ({ texts }: { texts: string[] }) => {
      dispatchTimes.push(Date.now());
      return createDelayedEmbedder(tracker, 0)({ texts });
    };
    const manager = createManager(tempHome(), {
      embedder,
      concurrency: 2,
      embeddingGate: { maxConcurrent: 2, minRequestIntervalMs: 80 },
    });
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    await importTextSource(manager, studioId, notebook.id, "事实一。");
    await importTextSource(manager, studioId, notebook.id, "事实二。");
    expect(await manager.ingestion.drainQueue()).toBe(2);
    expect(dispatchTimes.length).toBe(2);
    expect(dispatchTimes[1] - dispatchTimes[0]).toBeGreaterThanOrEqual(70);
  });
});

describe("Knowledge watcher 兼容（§七十）：共享源按 membership 各自 ensure 变体", () => {
  it("同源多笔记本不同分块配置 → refresh 后两个 profile 变体并存，不重复建工作", async () => {
    const manager = createManager(tempHome(), { embedder: createDelayedEmbedder({ active: 0, maxActive: 0, calls: 0 }, 0) });
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "A" });
    const notebookB = manager.createNotebook({ studioId, name: "B" });
    manager.updateNotebookSettings({ studioId, notebookId: notebookA.id, chunkTargetChars: 200, embeddingModelRef: FAKE_MODEL_REF });
    manager.updateNotebookSettings({ studioId, notebookId: notebookB.id, chunkTargetChars: 1200, embeddingModelRef: FAKE_MODEL_REF });

    // 模拟 watcher refresh 链路（refreshFileSource 对 pasted_text 不可用；直接走
    // 相同的入队序列）：触发笔记本 + 其余 membership 各自入队（去重后单 job/笔记本）。
    const { imported, artifact } = await importTextSource(
      manager, studioId, notebookA.id, "# 标题\n苹果项目的交付日期是九月十五日。\n# 结论\n火星项目的预算是八百万元。",
    );
    manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId: imported.source.id });
    const jobA2 = manager.enqueueSourceIngestion({ studioId, notebookId: notebookA.id, sourceId: imported.source.id, artifactId: artifact.id });
    const jobB = manager.enqueueSourceIngestion({ studioId, notebookId: notebookB.id, sourceId: imported.source.id, artifactId: artifact.id });
    // §十七 幂等：重复 watcher 事件（重复入队）不建重复工作。
    expect(manager.enqueueSourceIngestion({ studioId, notebookId: notebookA.id, sourceId: imported.source.id, artifactId: artifact.id }).id)
      .toBe(jobA2.id);

    expect(await manager.ingestion.drainQueue()).toBe(2);
    // 两个 (artifact, profile) 变体并存、互不覆盖（§七十：相同 Variant 共享，不同并存）。
    const variants = manager.indexStore.listChunkIndexVariantsByArtifact(artifact.id);
    expect(variants.length).toBe(2);
    expect(new Set(variants.map(variant => variant.chunkProfileHash)).size).toBe(2);
    for (const variant of variants) {
      expect(variant.status).toBe("ready");
      // 每个 profile 各自的向量变体就位。
      const vivs = manager.vectorIndex.listVariantsByChunkIndexVariant(variant.id);
      expect(vivs.length).toBe(1);
      expect(vivs[0].status).toBe("ready");
    }
    // 活跃 job 唯一性（v12 部分唯一索引）：每 (notebook, source) 一条。
    expect(countRows(
      manager,
      `SELECT COUNT(*) AS count FROM ingestion_jobs WHERE source_id = ? AND status IN ('queued','running','pending_embedding')`,
      imported.source.id,
    )).toBe(0); // 全部 done 后活跃态为 0。
    expect(jobB.id).not.toBe(jobA2.id);
  });
});

describe("Knowledge 零引用变体诊断（§十八 DerivedIndexVariant）", () => {
  it("零引用 chunk 变体只检测不清理；活跃 profile 的变体不进候选", async () => {
    const manager = createManager(tempHome(), { embedder: createDelayedEmbedder({ active: 0, maxActive: 0, calls: 0 }, 0) });
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const { imported, artifact } = await importTextSource(manager, studioId, notebook.id, "事实内容。");
    await manager.ingestion.drainQueue();
    // 换分块配置重建：旧 profile 的变体仍在（additive），但不再被任何笔记本引用。
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, chunkTargetChars: 100 });
    manager.enqueueNotebookRebuild({ studioId, notebookId: notebook.id });
    await manager.ingestion.drainQueue();

    const variants = manager.indexStore.listChunkIndexVariantsByArtifact(artifact.id);
    expect(variants.length).toBe(2);
    const candidates = manager.collectDerivedVariantGcCandidates();
    const candidateKeys = new Set(candidates.map(candidate => `${candidate.parseArtifactId}\0${candidate.chunkProfileHash}`));
    // 活跃（当前绑定）profile 的变体不在候选里；旧 profile 的变体在候选里且物理行保留。
    const activeProfileHash = chunkProfileHashOf(manager, studioId, artifact.id, 100);
    expect(candidateKeys.has(`${artifact.id}\0${activeProfileHash}`)).toBe(false);
    expect(candidates.some(candidate => candidate.parseArtifactId === artifact.id)).toBe(true);
    expect(manager.indexStore.listChunkIndexVariantsByArtifact(artifact.id).length).toBe(2);
    // 源本身未被 GC 误删（诊断面无删除行为）。
    expect(manager.getSource({ studioId, sourceId: imported.source.id }).id).toBe(imported.source.id);
  });
});
