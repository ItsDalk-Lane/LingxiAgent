import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";
import {
  KNOWLEDGE_FUSION_BUDGET,
  KNOWLEDGE_RERANK_MAX_DOCS,
} from "../lib/knowledge/knowledge-query-service.ts";
import type { KnowledgeParseArtifact } from "../lib/knowledge/types.ts";

/**
 * 回归：查询侧懒构建与摄入侧的分块配置同源。
 * 事故（2026-08-29）：查询侧 ensure 链按内置默认 1200 重建 FTS 索引，与摄入侧
 * 按笔记本生效值（显式列 > 嵌入模型上下文 ×80%）写入的 chunkerConfigId 失配，
 * 一次问答触发全量重嵌并在单槽本地嵌入上放大为超时风暴。
 */
const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-query-align-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

const FAKE_MODEL_REF = { id: "emb-1", provider: "fake" };
const FAKE_RERANK_REF = { id: "rer-1", provider: "fake" };

/** 8 维确定性伪嵌入：记录每次调用的文本批，供"是否重嵌"断言。 */
function createManager(lingxiHome: string, options: {
  contextWindow?: number | null;
  rerankResult?: "ok" | "unresolvable";
} = {}) {
  const embedCalls: string[][] = [];
  const rerankCalls: Array<{ query: string; docCount: number; modelRef: unknown }> = [];
  const manager = new KnowledgeManager({
    lingxiHome,
    embedTextsForModel: async (request) => {
      embedCalls.push([...request.texts]);
      return {
        vectors: request.texts.map((text) => {
          const vector = new Array(8).fill(0);
          vector[text.length % 8] = (text.length % 7) + 1;
          return vector;
        }),
        dimensions: 8,
        model: { provider: "fake", id: "emb-1", api: "openai", dimensions: 8 },
      };
    },
    canEmbedWithModel: () => true,
    rerankForModel: async (request) => {
      rerankCalls.push({ query: request.query, docCount: request.documents.length, modelRef: request.modelRef });
      if (options.rerankResult === "unresolvable") return null;
      return {
        results: request.documents.map((_, index) => ({ index, score: 1 - index / (request.documents.length + 1) })),
      };
    },
    getEmbeddingModelContextWindow: options.contextWindow != null
      ? () => options.contextWindow!
      : undefined,
  });
  managers.push(manager);
  return { manager, embedCalls, rerankCalls };
}

/** 每章 ~2000 字 × chapterCount 章：target 1200（softcap 1800）逐章二分、
 * target 5000（softcap 7500）整章一块——两种尺寸产出不同块数。 */
function novelText(chapterCount: number): string {
  const chapters: string[] = [];
  for (let index = 1; index <= chapterCount; index += 1) {
    const paragraph = "末日之后的城市在长夜里延伸，幸存者提着灯穿过废墟。".repeat(64);
    chapters.push(`第${index}章 长夜\n\n${paragraph}`);
  }
  return chapters.join("\n\n");
}

async function ingestSource(manager: KnowledgeManager, studioId: string, notebookId: string) {
  const imported = await manager.importPastedText({
    studioId,
    notebookId,
    text: novelText(6),
    displayName: "小说.txt",
  });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  manager.enqueueSourceIngestion({
    studioId,
    notebookId,
    sourceId: imported.source.id,
    artifactId: artifact.id,
  });
  await manager.ingestion.drainQueue();
  return artifact;
}

/**
 * 按 owning notebook 的 RetrievalProfile 锚定列出该 artifact 的索引 chunk
 * （schema v2：chunk 挂在 ChunkIndexVariant 上，不再有跨配置的 listArtifactChunks）。
 */
function listNotebookProfileChunks(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  artifact: KnowledgeParseArtifact,
) {
  const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
  const strategy = resolveKnowledgeChunkerConfig(blocks, {
    targetChars: manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId }),
  }).strategy;
  const { chunkProfile } = manager.store.resolveNotebookRetrievalProfile({ studioId, notebookId, strategy });
  const variant = manager.indexStore.resolveChunkIndexVariant(artifact.id, chunkProfile.profileHash);
  return variant ? manager.indexStore.listVariantChunks(variant.id) : [];
}

describe("查询侧分块配置与摄入侧同源", () => {
  it("显式 chunkTargetChars：摄入后检索不重建索引、不重嵌（只多 1 次查询嵌入）", async () => {
    const { manager, embedCalls } = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "小说" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 5000,
    });
    const artifact = await ingestSource(manager, studioId, notebook.id);

    const chunksAfterIngestion = listNotebookProfileChunks(manager, studioId, notebook.id, artifact);
    expect(chunksAfterIngestion.length).toBe(6); // 5000：整章一块
    expect(embedCalls.length).toBe(1); // 摄入批量嵌入 6 块
    expect(embedCalls[0].length).toBe(6);

    await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "幸存者在做什么",
    });

    // 修复前：查询侧按默认 1200 重建（12 块）并对全部块重嵌；
    // 修复后：索引原样命中，只多出 1 次查询嵌入。
    expect(listNotebookProfileChunks(manager, studioId, notebook.id, artifact).length).toBe(6);
    expect(embedCalls.length).toBe(2);
    expect(embedCalls[1].length).toBe(1);
  });

  it("自动分块（null）：查询侧经上下文窗口回调解析出与摄入侧相同的生效值", async () => {
    // 40960 × 0.8 = 32768 > 7500：整章一块，与显式 5000 同型。
    const { manager, embedCalls } = createManager(tempHome(), { contextWindow: 40960 });
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "小说" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
    });
    const artifact = await ingestSource(manager, studioId, notebook.id);
    expect(listNotebookProfileChunks(manager, studioId, notebook.id, artifact).length).toBe(6);

    await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "长夜",
    });

    expect(listNotebookProfileChunks(manager, studioId, notebook.id, artifact).length).toBe(6);
    expect(embedCalls.length).toBe(2);
    expect(embedCalls[1].length).toBe(1);
  });

  it("向量未就绪：并行查询降级 FTS + 幂等入队后台构建，绝不现场批量嵌入（§十一/§十二）", async () => {
    const { manager, embedCalls } = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "小说" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 5000,
    });
    await ingestSource(manager, studioId, notebook.id);
    expect(embedCalls.length).toBe(1);

    // 模拟向量缺失（FTS 完好）：查询不得现场批量嵌入（Phase 2 前的懒构建已拆除）。
    manager.vectorIndex.rebuild();
    const callsBeforeRetrieve = embedCalls.length;

    const [first, second] = await Promise.all([
      manager.queryService.retrieveForNotebooks({ studioId, notebookIds: [notebook.id], question: "废墟" }),
      manager.queryService.retrieveForNotebooks({ studioId, notebookIds: [notebook.id], question: "幸存者" }),
    ]);

    // 查询线程只嵌入问题文本（各 1 条），零批量 chunk 嵌入。
    const batchCalls = embedCalls.slice(callsBeforeRetrieve).filter(call => call.length > 1);
    expect(batchCalls.length).toBe(0);
    for (const result of [first, second]) {
      expect(result.retrievalMode).toBe("fts");
      expect(result.retrievalModeRequested).toBe("hybrid");
      expect(result.degraded.some(entry => entry.reason === "KNOWLEDGE_VECTOR_NOT_READY")).toBe(true);
    }
    // 幂等入队：并行两次查询共享同一个活跃后台构建 job（活跃 job 去重）。
    expect(manager.store.listIngestionJobs({
      studioId,
      notebookId: notebook.id,
      statuses: ["queued", "running", "pending_embedding"],
    })).toHaveLength(1);

    // 后台构建完成后再次查询恢复 hybrid，降级清单清空。
    await manager.ingestion.drainQueue();
    const rebuilt = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "废墟",
    });
    expect(rebuilt.retrievalMode).toBe("hybrid");
    expect(rebuilt.degraded).toEqual([]);
    expect(manager.vectorIndex.health().status).toBe("ready");
  });

  it("rerank 按笔记本引用路由：可解析引用执行重排、不可解析引用显式降级 RRF 不失败", async () => {
    // 事故（2026-08-29 下午）：全局 rerank 槽 v8 退役后查询侧仍走全局解析，
    // 笔记本配置了失效引用 → 一次 rerank 解析错误连坐整个检索（KNOWLEDGE_RETRIEVAL_UNAVAILABLE）。
    const ok = createManager(tempHome());
    const studioId = "studio-a";
    const okNotebook = ok.manager.createNotebook({ studioId, name: "可用重排" });
    ok.manager.updateNotebookSettings({
      studioId,
      notebookId: okNotebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 5000,
      rerankModelRef: FAKE_RERANK_REF,
    });
    const okArtifact = await ingestSource(ok.manager, studioId, okNotebook.id);
    const okResult = await ok.manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [okNotebook.id],
      question: "幸存者",
    });
    expect(okResult.candidates.length).toBeGreaterThan(0);
    expect(okResult.retrievalMode).toBe("hybrid");
    expect(ok.rerankCalls.length).toBe(1);
    expect(ok.rerankCalls[0].modelRef).toEqual(FAKE_RERANK_REF);
    expect(ok.rerankCalls[0].docCount).toBe(listNotebookProfileChunks(ok.manager, studioId, okNotebook.id, okArtifact).length > 0 ? okResult.candidates.length : 0);

    // 引用不可解析（如模型被移出清单）：回调返回 null → 检索降级 RRF 名次而非失败。
    const degraded = createManager(tempHome(), { rerankResult: "unresolvable" });
    const degradedNotebook = degraded.manager.createNotebook({ studioId, name: "失效重排" });
    degraded.manager.updateNotebookSettings({
      studioId,
      notebookId: degradedNotebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 5000,
      rerankModelRef: FAKE_RERANK_REF,
    });
    await ingestSource(degraded.manager, studioId, degradedNotebook.id);
    const degradedResult = await degraded.manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [degradedNotebook.id],
      question: "幸存者",
    });
    expect(degradedResult.candidates.length).toBeGreaterThan(0);
    expect(degradedResult.retrievalMode).toBe("hybrid");
    expect(degraded.rerankCalls.length).toBe(1); // 尝试过、显式降级（调用侧记日志）
  });

  it("无上限召回（retrieval_top_k NULL）：预算链独立生效截断候选，重排输入不超共享上限 100，检索不失败", async () => {
    // 事故（2026-08-29 第二连）回归锚 + §二十六（Phase 8）：topK=NULL→1000 不再
    // 作为覆盖机制——候选生成（每通道）与融合池按 candidate budgets 独立截断；
    // rerank 输入仍受共享上限保护（docCount ≤ 100），两侧常量不再打架。
    const { manager, rerankCalls } = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "长文" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 5000,
      rerankModelRef: FAKE_RERANK_REF,
    });
    const chapters: string[] = [];
    for (let index = 1; index <= 150; index += 1) {
      chapters.push(`第${index}章 长夜\n\n${"废墟之下的长夜灯火绵延，幸存者默数着白昼。".repeat(40)}（第${index}节）`);
    }
    const imported = await manager.importPastedText({
      studioId,
      notebookId: notebook.id,
      text: chapters.join("\n\n"),
      displayName: "长文.txt",
    });
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    manager.enqueueSourceIngestion({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
      artifactId: artifact.id,
    });
    await manager.ingestion.drainQueue();
    expect(listNotebookProfileChunks(manager, studioId, notebook.id, artifact).length).toBe(150);

    // retrieval_top_k 未配置 = 无上限召回：150 个可匹配 chunk 在融合池处被
    // fusionBudget 截断（预算链独立于 topK 生效），绝不再冲到物理边界 1000。
    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "长夜",
    });
    expect(result.candidates.length).toBe(KNOWLEDGE_FUSION_BUDGET);
    expect(result.candidates.length).toBeLessThanOrEqual(KNOWLEDGE_RERANK_MAX_DOCS);
    expect(rerankCalls.length).toBe(1);
    expect(rerankCalls[0].docCount).toBeLessThanOrEqual(KNOWLEDGE_RERANK_MAX_DOCS); // 裁剪到共享上限
  });
});
