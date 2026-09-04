import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import {
  KNOWLEDGE_CHUNK_TARGET_CHARS,
  knowledgeBlockFingerprint,
  resolveKnowledgeChunkerConfig,
} from "../lib/knowledge/chunker.ts";
import { knowledgeChunkIndexVariantId } from "../lib/knowledge/knowledge-index-store.ts";
import { knowledgeVectorIndexVariantId } from "../lib/knowledge/vector-index-adapter.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-ingestion-"));
  tempDirs.push(dir);
  return dir;
}

function untrack(manager: KnowledgeManager) {
  const index = managers.indexOf(manager);
  if (index >= 0) managers.splice(index, 1);
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

interface EmbeddingHarness {
  embedder: ((request: { texts: string[] }) => Promise<any>) | null;
  calls: string[][];
}

/** 8 维确定性伪嵌入：向量由文本长度导出，足以通过向量库校验并产生可检索结果。 */
function createFakeEmbedder(harness: EmbeddingHarness) {
  return async ({ texts }: { texts: string[] }) => {
    harness.calls.push([...texts]);
    return {
      vectors: texts.map((text) => {
        const vector = new Array(8).fill(0);
        vector[text.length % 8] = (text.length % 7) + 1;
        return vector;
      }),
      dimensions: 8,
      model: { provider: "fake", id: "emb-1", api: "openai", dimensions: 8 },
    };
  };
}

const FAKE_MODEL_REF = { id: "emb-1", provider: "fake" };

/**
 * 模拟"笔记本配置了嵌入模型"（v8 起笔记本列是唯一来源）：harness 建的
 * 笔记本写入 FAKE_MODEL_REF；可解析性由 canEmbedWithModel 随 embedder
 * 置位动态联动（与旧全局偏好语义一致）。
 */
function createManager(lingxiHome: string, options: { now?: () => string } = {}) {
  const embedding: EmbeddingHarness = { embedder: null, calls: [] };
  const manager = new KnowledgeManager({
    lingxiHome,
    now: options.now,
    embedTextsForModel: (request) => {
      if (!embedding.embedder) return Promise.resolve(null);
      return embedding.embedder(request);
    },
    canEmbedWithModel: () => embedding.embedder !== null,
  });
  managers.push(manager);
  return { manager, embedding };
}

/** 与路由 POST sources 相同的调用序列：导入 → 解析 → 入队。 */
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

function getJob(manager: KnowledgeManager, studioId: string, jobId: string) {
  return manager.store.getIngestionJob({ studioId, jobId });
}

function shiftNow(holder: { value: string }, ms: number) {
  holder.value = new Date(Date.parse(holder.value) + ms).toISOString();
}

describe("Knowledge 摄入管线", () => {
  it("导入即入队；无嵌入模型时 parse+chunk+FTS 完成后落 pending_embedding（FTS 可查）", async () => {
    const { manager } = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const { artifact, job } = await importTextSource(
      manager,
      studioId,
      notebook.id,
      "苹果项目的交付日期是九月十五日。\n火星项目的预算是八百万元。",
    );
    expect(job).toMatchObject({ status: "queued", phase: "parse", artifactId: artifact.id });
    // 入队即记录触发方笔记本的分块配置指纹（按真实 blocks 计算）。
    const blocks = manager.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
    expect(job.chunkerConfigId).toBe(resolveKnowledgeChunkerConfig(blocks, { targetChars: KNOWLEDGE_CHUNK_TARGET_CHARS }).configId);
    // 活跃 job 去重：重复入队返回同一 job。
    expect(manager.enqueueSourceIngestion({
      studioId,
      notebookId: notebook.id,
      sourceId: job.sourceId,
      artifactId: artifact.id,
    }).id).toBe(job.id);

    expect(await manager.ingestion.drainQueue()).toBe(1);
    const finished = getJob(manager, studioId, job.id);
    expect(finished).toMatchObject({ status: "pending_embedding", phase: "embed", artifactId: artifact.id });

    // pending_embedding 是显式终态而非不可用：FTS 已可检索。
    // 检索锚（schema v2）：(parseArtifactId, chunkProfileHash)，hash 即上面的 chunkerConfigId。
    const hits = manager.indexStore.search({
      scopes: [{
        parseArtifactId: artifact.id,
        chunkProfileHash: resolveKnowledgeChunkerConfig(blocks, { targetChars: KNOWLEDGE_CHUNK_TARGET_CHARS }).configId,
      }],
      query: "交付日期",
      limit: 12,
    });
    expect(hits.length).toBeGreaterThan(0);
    expect(manager.store.countIngestionJobsByStatus({ studioId, notebookId: notebook.id }))
      .toMatchObject({ pending_embedding: 1, failed: 0 });
  });

  it("嵌入可解析时摄入到 done，向量可检索；重复摄入 embed 相位幂等跳过", async () => {
    const { manager, embedding } = createManager(tempHome());
    embedding.embedder = createFakeEmbedder(embedding);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const { artifact, job } = await importTextSource(
      manager,
      studioId,
      notebook.id,
      "苹果项目的交付日期是九月十五日。",
    );

    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(getJob(manager, studioId, job.id)).toMatchObject({ status: "done", phase: "done" });
    expect(embedding.calls).toHaveLength(1); // 小文本单批

    // 向量库确实写入了该 artifact 的向量（锚点：viv = f(civ, modelKey) 确定性派生）。
    const modelKey = crypto.createHash("sha256")
      .update(JSON.stringify(["fake", "emb-1", "openai", 8]), "utf8")
      .digest("hex");
    const chunkProfileHash = resolveKnowledgeChunkerConfig(
      manager.listArtifactBlocks({ studioId, parseArtifactId: artifact.id }),
      { targetChars: KNOWLEDGE_CHUNK_TARGET_CHARS },
    ).configId;
    const vectorHits = manager.vectorIndex.search({
      vectorIndexVariantIds: [
        knowledgeVectorIndexVariantId(knowledgeChunkIndexVariantId(artifact.id, chunkProfileHash), modelKey),
      ],
      model: { key: modelKey, provider: "fake", modelId: "emb-1", protocol: "openai", dimensions: 8 },
      queryVector: [1, 0, 0, 0, 0, 0, 0, 0],
      limit: 5,
    });
    expect(vectorHits.length).toBeGreaterThan(0);

    // 重复摄入（配置未变）：chunk/FTS 指纹命中跳过；embed 相位第一批确认
    // 模型身份后 hasArtifact 命中跳过——只多跑一个探测批，不整源重嵌。
    manager.enqueueSourceIngestion({
      studioId,
      notebookId: notebook.id,
      sourceId: job.sourceId,
      artifactId: artifact.id,
    });
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(embedding.calls).toHaveLength(2);
    expect(manager.store.countIngestionJobsByStatus({ studioId })).toMatchObject({ done: 2 });
  });

  it("崩溃恢复：running 残留被重置回 queued 并续跑完成", async () => {
    const home = tempHome();
    const studioId = "studio-a";
    const first = createManager(home);
    first.embedding.embedder = createFakeEmbedder(first.embedding);
    const notebook = first.manager.createNotebook({ studioId, name: "资料" });
    first.manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const { job } = await importTextSource(
      first.manager,
      studioId,
      notebook.id,
      "崩溃恢复语料：第一节内容。\n第二节内容。",
    );

    // 模拟崩溃：job 被认领为 running 后进程中断（队列从未 start）。
    const claimed = first.manager.store.claimNextIngestionJob();
    expect(claimed).toMatchObject({ id: job.id, status: "running" });
    untrack(first.manager);
    first.manager.close();

    // 重启：启动恢复把 running 残留置回 queued，drain 续跑（相位幂等）到 done。
    const restarted = createManager(home);
    restarted.embedding.embedder = createFakeEmbedder(restarted.embedding);
    expect(restarted.manager.ingestion.recoverInterruptedJobs()).toBe(1);
    expect(getJob(restarted.manager, studioId, job.id).status).toBe("queued");
    expect(await restarted.manager.ingestion.drainQueue()).toBe(1);
    expect(getJob(restarted.manager, studioId, job.id)).toMatchObject({ status: "done", phase: "done" });
    expect(restarted.embedding.calls.length).toBeGreaterThan(0);
  });

  it("模型就绪信号：可解析才把 pending_embedding 置回 queued 并补跑到 done", async () => {
    const { manager, embedding } = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const { job } = await importTextSource(
      manager,
      studioId,
      notebook.id,
      "待嵌入语料：配置嵌入模型后应自动补跑。",
    );
    await manager.ingestion.drainQueue();
    expect(getJob(manager, studioId, job.id).status).toBe("pending_embedding");

    // 模型仍未配置：就绪信号不补跑（避免空转），状态保持 pending_embedding。
    expect(manager.onModelConfigMayHaveChanged()).toBe(0);
    expect(getJob(manager, studioId, job.id).status).toBe("pending_embedding");

    // 模型配置好：批量置回 queued，drain 补跑 embed 相位到 done。
    embedding.embedder = createFakeEmbedder(embedding);
    expect(manager.onModelConfigMayHaveChanged()).toBe(1);
    expect(getJob(manager, studioId, job.id).status).toBe("queued");
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(getJob(manager, studioId, job.id)).toMatchObject({ status: "done", phase: "done" });
    expect(embedding.calls).toHaveLength(1);
  });

  it("笔记本配置变更：分块/嵌入字段触发全量重建，检索字段不触发", async () => {
    const { manager, embedding } = createManager(tempHome());
    embedding.embedder = createFakeEmbedder(embedding);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const first = await importTextSource(manager, studioId, notebook.id, "甲项目的交付日期是九月。", "甲.txt");
    const second = await importTextSource(manager, studioId, notebook.id, "乙项目的预算是八百万元。", "乙.txt");
    expect(await manager.ingestion.drainQueue()).toBe(2);
    const embedCallsAfterInitial = embedding.calls.length;

    // 只改检索参数（retrievalTopK）：不影响派生产物，不重建。
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, retrievalTopK: 8 });
    expect(manager.store.listIngestionJobs({
      studioId,
      notebookId: notebook.id,
      statuses: ["queued", "running", "pending_embedding"],
    })).toHaveLength(0);

    // 改分块尺寸：两个源全部重新入队，重建后 FTS 新变体 ready（新契约实参顺序：
    // (parseArtifactId, chunkProfileHash, fingerprint)；旧变体共存不被覆盖）。
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, chunkTargetChars: 300 });
    expect(manager.store.listIngestionJobs({
      studioId,
      notebookId: notebook.id,
      statuses: ["queued"],
    })).toHaveLength(2);
    expect(await manager.ingestion.drainQueue()).toBe(2);
    for (const entry of [first, second]) {
      const blocks = manager.listArtifactBlocks({ studioId, parseArtifactId: entry.artifact.id });
      const configId = resolveKnowledgeChunkerConfig(blocks, { targetChars: 300 }).configId;
      expect(manager.indexStore.hasArtifactFingerprint(
        entry.artifact.id,
        configId,
        knowledgeBlockFingerprint(blocks),
      )).toBe(true);
    }
    // chunk 指纹变化 → 向量按新 chunk 重嵌（不静默沿用旧向量）。
    expect(embedding.calls.length).toBeGreaterThan(embedCallsAfterInitial);

    // 改嵌入模型引用：同样触发全量重建。
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: { id: "emb-2", provider: "fake" },
    });
    expect(manager.store.listIngestionJobs({
      studioId,
      notebookId: notebook.id,
      statuses: ["queued"],
    })).toHaveLength(2);
  });

  it("失败重试：30s/120s/600s 指数退避，3 次重试后标 failed（显式终态）", async () => {
    const now = { value: "2026-08-28T00:00:00.000Z" };
    const { manager, embedding } = createManager(tempHome(), { now: () => now.value });
    embedding.embedder = async () => {
      throw new TypeError("network down"); // 可重试错误（被包装为 KNOWLEDGE_RETRIEVAL_UNAVAILABLE）
    };
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const { job } = await importTextSource(manager, studioId, notebook.id, "退避测试语料。");

    const backoffs = [30_000, 120_000, 600_000];
    for (let attempt = 0; attempt < backoffs.length; attempt += 1) {
      expect(await manager.ingestion.drainQueue()).toBe(1);
      const failedOnce = getJob(manager, studioId, job.id);
      expect(failedOnce).toMatchObject({ status: "queued", attempt: attempt + 1 });
      expect(failedOnce.error).toContain("KNOWLEDGE_RETRIEVAL_UNAVAILABLE");
      expect(Date.parse(failedOnce.retryAfter!)).toBe(Date.parse(now.value) + backoffs[attempt]);
      // 退避未到期：claim 不到任何 job。
      expect(await manager.ingestion.drainQueue()).toBe(0);
      shiftNow(now, backoffs[attempt] + 1000);
    }

    // attempt 已达上限：第 4 次失败标 failed，不再排队。
    expect(await manager.ingestion.drainQueue()).toBe(1);
    const failed = getJob(manager, studioId, job.id);
    expect(failed).toMatchObject({ status: "failed", attempt: 4, retryAfter: null });
    expect(manager.store.countIngestionJobsByStatus({ studioId })).toMatchObject({ failed: 1 });

    // 手动重试（UI 路径）：attempt 归零、从失败的 embed 相位续跑。
    manager.store.requeueIngestionJob({ studioId, jobId: job.id });
    embedding.embedder = createFakeEmbedder(embedding);
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(getJob(manager, studioId, job.id)).toMatchObject({ status: "done", attempt: 0 });
  });

  it("永久性错误（解析失败）直接标 failed，不消耗退避重试", async () => {
    const home = tempHome();
    const outside = path.join(home, "..", `${path.basename(home)}-outside`);
    fs.mkdirSync(outside, { recursive: true });
    tempDirs.push(outside);
    const fakePdf = path.join(outside, "broken.pdf");
    fs.writeFileSync(fakePdf, Buffer.concat([Buffer.from("%PDF-"), Buffer.from("不是合法 PDF 内容")]));

    const { manager, embedding } = createManager(home);
    embedding.embedder = createFakeEmbedder(embedding);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const imported = await manager.importFile({
      studioId,
      notebookId: notebook.id,
      filePath: fakePdf,
    });
    // 路由解析失败分支同款：不带 artifactId 入队，worker 从 parse 相位起跑。
    const job = manager.enqueueSourceIngestion({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
    });
    expect(job).toMatchObject({ status: "queued", phase: "parse", artifactId: null });

    expect(await manager.ingestion.drainQueue()).toBe(1);
    const failed = getJob(manager, studioId, job.id);
    expect(failed).toMatchObject({ status: "failed", attempt: 1, retryAfter: null });
    expect(failed.error).toContain("KNOWLEDGE_PARSE_FAILED");
    expect(embedding.calls).toHaveLength(0);
  });
});

describe("Embedding 批级 checkpoint 恢复（§九十，Phase 3）", () => {
  /**
   * 100 chunk 语料：text 章节策略，100 行 "第N章 …"（每行一章一节一 chunk），
   * 每行 120 字符 ≤ softCap(targetChars×1.5=150)，合计恰好 100 个 chunk（两批：64 + 36）。
   */
  function hundredChunkText(): string {
    return Array.from({ length: 100 }, (_, index) => (
      `第${index + 1}章恢复测试语料`.padEnd(120, "填")
    )).join("\n");
  }

  function variantHandle(manager: KnowledgeManager, artifactId: string) {
    const blocks = manager.listArtifactBlocks({ studioId: "studio-a", parseArtifactId: artifactId });
    const chunkProfileHash = resolveKnowledgeChunkerConfig(blocks, { targetChars: 100 }).configId;
    const civ = knowledgeChunkIndexVariantId(artifactId, chunkProfileHash);
    const modelKey = crypto.createHash("sha256")
      .update(JSON.stringify(["fake", "emb-1", "openai", 8]), "utf8")
      .digest("hex");
    return { civ, viv: knowledgeVectorIndexVariantId(civ, modelKey), chunkProfileHash };
  }

  function vectorCount(manager: KnowledgeManager, viv: string): number {
    return manager.vectorIndex.db.prepare(
      `SELECT COUNT(*) AS count FROM chunk_vectors WHERE vector_index_variant_id = ?`,
    ).get(viv).count;
  }

  it("100 chunks：第二批失败后 64 块已落库，重试只补 65–100，变体 ready、向量 100", async () => {
    // 注：checkpoint 以 64 块/批为原子单位，§九十 的 80/20 在本实现映射为 64/36
    // （第一批 64 块持久化后中断）；断言的核心不变：已落库块绝不重嵌。
    const now = { value: "2026-08-28T00:00:00.000Z" };
    const { manager, embedding } = createManager(tempHome(), { now: () => now.value });
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 100,
    });
    embedding.embedder = async ({ texts }: { texts: string[] }) => {
      embedding.calls.push([...texts]);
      if (embedding.calls.length === 2) throw new TypeError("network down"); // 第二批失败
      return {
        vectors: texts.map((text) => {
          const vector = new Array(8).fill(0);
          vector[text.length % 8] = (text.length % 7) + 1;
          return vector;
        }),
        dimensions: 8,
        model: { provider: "fake", id: "emb-1", api: "openai", dimensions: 8 },
      };
    };
    const { artifact, job } = await importTextSource(manager, studioId, notebook.id, hundredChunkText());

    expect(await manager.ingestion.drainQueue()).toBe(1);
    const { viv, civ } = variantHandle(manager, artifact.id);
    // 分块 sanity：确实 100 块（两批：64 + 36；chunk 相位已建出，embed 在第二批失败）。
    expect(manager.indexStore.listVariantChunks(civ)).toHaveLength(100);
    const failedOnce = getJob(manager, studioId, job.id);
    expect(failedOnce).toMatchObject({ status: "queued", attempt: 1 });
    expect(failedOnce.error).toContain("KNOWLEDGE_RETRIEVAL_UNAVAILABLE");
    // 失败保留进度（不再清零）：64/100 是真实已落库的 checkpoint。
    expect(failedOnce).toMatchObject({ progressDone: 64, progressTotal: 100 });
    expect(embedding.calls).toHaveLength(2);
    expect(embedding.calls[0]).toHaveLength(64);
    // 批级 checkpoint：第一批 64 块已在向量库，variant 保持 building（不是重头再来）。
    expect(vectorCount(manager, viv)).toBe(64);
    expect(manager.vectorIndex.getVariant(viv)).toMatchObject({ status: "building" });

    // 退避到期后重试：免探测恢复（唯一 building 断点变体），只嵌缺失的 36 块。
    embedding.embedder = createFakeEmbedder(embedding);
    shiftNow(now, 31_000);
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(getJob(manager, studioId, job.id)).toMatchObject({ status: "done", phase: "done" });
    const resumeCalls = embedding.calls.slice(2);
    expect(resumeCalls).toHaveLength(1);
    const expectedMissing = manager.indexStore.listVariantChunks(civ).slice(64).map(chunk => chunk.text);
    expect(resumeCalls[0]).toEqual(expectedMissing); // 恰好 65–100，1–64 未重嵌
    expect(resumeCalls[0]).toHaveLength(36);
    expect(manager.vectorIndex.getVariant(viv)).toMatchObject({ status: "ready" });
    expect(vectorCount(manager, viv)).toBe(100);

    // 成本观测（§七十四）落 job：本轮新嵌 36、断点续用 64、一次请求。
    expect(getJob(manager, studioId, job.id).embeddingStats).toMatchObject({
      chunksNewlyEmbedded: 36,
      chunksResumedFromCheckpoint: 64,
      chunksReusedFromReadyVariant: 0,
      requestCount: 1,
      model: { provider: "fake", modelId: "emb-1", protocol: "openai", dimensions: 8 },
      resetStaleVectors: false,
      abandonedStaleVariantId: null,
    });
  });

  it("中断重启：stop() 中断 embed 后换进程（重建 manager），恢复只补缺失块且留痕 INTERRUPTED", async () => {
    const home = tempHome();
    const studioId = "studio-a";
    const first = createManager(home);
    const notebook = first.manager.createNotebook({ studioId, name: "资料" });
    first.manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 100,
    });
    first.embedding.embedder = async ({ texts, signal }: { texts: string[]; signal?: AbortSignal }) => {
      first.embedding.calls.push([...texts]);
      if (first.embedding.calls.length === 2) {
        // 第二批进行中进程收到停止信号（stop 中断语义）。
        first.manager.ingestion.stop();
        expect(signal?.aborted).toBe(true);
        throw Object.assign(new Error("aborted"), { name: "AbortError" });
      }
      return {
        vectors: texts.map((text) => {
          const vector = new Array(8).fill(0);
          vector[text.length % 8] = (text.length % 7) + 1;
          return vector;
        }),
        dimensions: 8,
        model: { provider: "fake", id: "emb-1", api: "openai", dimensions: 8 },
      };
    };
    const { artifact, job } = await importTextSource(
      first.manager, studioId, notebook.id, hundredChunkText(),
    );
    const { viv } = variantHandle(first.manager, artifact.id);

    await first.manager.ingestion.drainQueue();
    const interrupted = getJob(first.manager, studioId, job.id);
    // stop 中断：不消耗 attempt、置回 queued、embed 相位显式留痕（§一百零四）。
    expect(interrupted).toMatchObject({ status: "queued", attempt: 0, progressDone: 64, progressTotal: 100 });
    expect(interrupted.error).toContain("KNOWLEDGE_EMBEDDING_INTERRUPTED");
    expect(vectorCount(first.manager, viv)).toBe(64);
    untrack(first.manager);
    first.manager.close();

    // 模拟进程重启：新 manager 打开同一 LINGXI_HOME，断点向量仍在，只补 65–100。
    const restarted = createManager(home);
    restarted.embedding.embedder = createFakeEmbedder(restarted.embedding);
    expect(await restarted.manager.ingestion.drainQueue()).toBe(1);
    expect(getJob(restarted.manager, studioId, job.id)).toMatchObject({ status: "done", phase: "done" });
    expect(restarted.embedding.calls).toHaveLength(1);
    expect(restarted.embedding.calls[0]).toHaveLength(36);
    expect(restarted.manager.vectorIndex.getVariant(viv)).toMatchObject({ status: "ready" });
    expect(vectorCount(restarted.manager, viv)).toBe(100);
  });

  it("分块配置变更：新 profile 建独立 variant，旧 variant 向量不混不丢", async () => {
    const { manager, embedding } = createManager(tempHome());
    embedding.embedder = createFakeEmbedder(embedding);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 100,
    });
    const { artifact } = await importTextSource(manager, studioId, notebook.id, hundredChunkText());
    expect(await manager.ingestion.drainQueue()).toBe(1);
    const firstVariant = variantHandle(manager, artifact.id);
    expect(vectorCount(manager, firstVariant.viv)).toBe(100);
    expect(manager.vectorIndex.getVariant(firstVariant.viv)).toMatchObject({ status: "ready" });

    // chunk 内容/配置变化 → 新 civ → 新 variant 独立构建，旧 variant 原样保留。
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, chunkTargetChars: 300 });
    expect(await manager.ingestion.drainQueue()).toBe(1);
    const blocks = manager.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
    const newProfileHash = resolveKnowledgeChunkerConfig(blocks, { targetChars: 300 }).configId;
    const modelKey = crypto.createHash("sha256")
      .update(JSON.stringify(["fake", "emb-1", "openai", 8]), "utf8")
      .digest("hex");
    const newCiv = knowledgeChunkIndexVariantId(artifact.id, newProfileHash);
    const newViv = knowledgeVectorIndexVariantId(newCiv, modelKey);
    expect(newViv).not.toBe(firstVariant.viv);
    expect(manager.vectorIndex.getVariant(newViv)).toMatchObject({ status: "ready" });
    // 章节语料在 targetChars=300 下仍一章一块（120 ≤ softCap 450）：chunk 数同为 100，
    // 但 chunk id 由新 configId 派生，两个 variant 各自持有独立向量行。
    expect(vectorCount(manager, newViv)).toBe(100);
    const overlap = manager.vectorIndex.db.prepare(`
      SELECT COUNT(*) AS count FROM chunk_vectors old
      JOIN chunk_vectors new ON old.chunk_id = new.chunk_id
      WHERE old.vector_index_variant_id = ? AND new.vector_index_variant_id = ?
    `).get(firstVariant.viv, newViv).count;
    expect(overlap).toBe(0); // 不同 profile 的 chunk 身份零交叉（不混写）
    // 旧 variant：向量不丢不混（付费产物保留，供旧 profile 检索/后续 GC 显式处理）。
    expect(vectorCount(manager, firstVariant.viv)).toBe(100);
    expect(manager.vectorIndex.getVariant(firstVariant.viv)).toMatchObject({ status: "ready" });
  });
});
describe("向量保留策略与删除清理", () => {
  it("sweep：超期未使用的旧身份被回收，最新身份与查询命中者保留", async () => {
    const home = tempHome();
    const now = { value: "2026-08-01T00:00:00.000Z" };
    const { manager, embedding } = createManager(home, { now: () => now.value });
    embedding.embedder = createFakeEmbedder(embedding);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      vectorRetentionDays: 7,
    });
    const { artifact, job } = await importTextSource(manager, studioId, notebook.id, "雪花写作法第一章内容示例。".repeat(40));
    expect(await manager.ingestion.drainQueue()).toBe(1);
    expect(getJob(manager, studioId, job.id).status).toBe("done");

    // 手工塞入一份"历史模型身份"向量（模拟换模型后的作废副本），并拨老其使用时间。
    const staleIdentity = {
      key: "fake/emb-old/openai/8",
      provider: "fake",
      modelId: "emb-old",
      protocol: "openai",
      dimensions: 8,
    };
    manager.vectorIndex.buildOrReplaceArtifact({
      parseArtifactId: artifact.id,
      chunkFingerprint: "legacy-fingerprint",
      model: staleIdentity,
      entries: [{ chunkId: "stale-chunk", parseArtifactId: artifact.id, ordinal: 0, vector: new Array(8).fill(0.5) }],
    });
    manager.vectorIndex.db.prepare(`UPDATE vector_index_variants SET last_used_at = ? WHERE model_key = ?`)
      .run("2026-07-28T00:00:00.000Z", staleIdentity.key);

    // 未超期（距 now 4 天 < 7 天）：不删。
    expect(manager.queryService.sweepStaleVectorArtifacts({ now: () => now.value })).toBe(0);
    // 30 天后：旧身份超期回收，当前身份保留。
    const later = { value: "2026-09-01T00:00:00.000Z" };
    expect(manager.queryService.sweepStaleVectorArtifacts({ now: () => later.value })).toBe(1);
    const remaining = manager.vectorIndex.listArtifactUsage();
    expect(remaining.map((row) => row.modelKey)).toHaveLength(1);
    expect(remaining[0].modelKey).not.toBe(staleIdentity.key);
  });

  it("未配置保留策略（默认）时 sweep 不删任何向量", async () => {
    const home = tempHome();
    const { manager, embedding } = createManager(home);
    embedding.embedder = createFakeEmbedder(embedding);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const { artifact } = await importTextSource(manager, studioId, notebook.id, "默认永久保留策略下的内容示例。".repeat(40));
    await manager.ingestion.drainQueue();

    const staleIdentity = {
      key: "fake/emb-old/openai/8",
      provider: "fake",
      modelId: "emb-old",
      protocol: "openai",
      dimensions: 8,
    };
    manager.vectorIndex.buildOrReplaceArtifact({
      parseArtifactId: artifact.id,
      chunkFingerprint: "legacy-fingerprint",
      model: staleIdentity,
      entries: [{ chunkId: "stale-chunk", parseArtifactId: artifact.id, ordinal: 0, vector: new Array(8).fill(0.5) }],
    });
    manager.vectorIndex.db.prepare(`UPDATE vector_index_variants SET last_used_at = ? WHERE model_key = ?`)
      .run("2020-01-01T00:00:00.000Z", staleIdentity.key);
    expect(manager.queryService.sweepStaleVectorArtifacts({ now: () => "2026-09-01T00:00:00.000Z" })).toBe(0);
    expect(manager.vectorIndex.listArtifactUsage()).toHaveLength(2);
  });

  it("删除唯一笔记本挂靠的源时清理其派生索引；仍挂靠其他笔记本时不清理", async () => {
    const home = tempHome();
    const { manager, embedding } = createManager(home);
    embedding.embedder = createFakeEmbedder(embedding);
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "甲" });
    const notebookB = manager.createNotebook({ studioId, name: "乙" });
    manager.updateNotebookSettings({ studioId, notebookId: notebookA.id, embeddingModelRef: FAKE_MODEL_REF });
    const { imported, artifact } = await importTextSource(manager, studioId, notebookA.id, "待删除源的内容示例。".repeat(40));
    await manager.ingestion.drainQueue();
    manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId: imported.source.id });

    // 从甲移除但乙仍挂靠：不清理。
    manager.removeSourceFromNotebook({ studioId, notebookId: notebookA.id, sourceId: imported.source.id });
    expect(manager.vectorIndex.listArtifactUsage().some((row) => row.parseArtifactId === artifact.id)).toBe(true);

    // 从乙也移除（孤儿）：向量与 FTS 行全部清理。
    manager.removeSourceFromNotebook({ studioId, notebookId: notebookB.id, sourceId: imported.source.id });
    expect(manager.vectorIndex.listArtifactUsage().some((row) => row.parseArtifactId === artifact.id)).toBe(false);
    expect(manager.indexStore.listArtifactChunks(artifact.id)).toHaveLength(0);
  });
});
