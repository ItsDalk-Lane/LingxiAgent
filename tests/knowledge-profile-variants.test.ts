import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";
import { knowledgeChunkIndexVariantId } from "../lib/knowledge/knowledge-index-store.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { knowledgeVectorIndexVariantId } from "../lib/knowledge/vector-index-adapter.ts";
import { estimateTextTokens } from "../lib/llm/estimate-text-tokens.ts";

/**
 * 任务书 §八十九 Profile-aware Index（本阶段场景 ①②③⑦）：
 * ① 同一 Source 属于两个 Notebook、chunk 配置不同 → 两个 ChunkIndexVariant 并存互不覆盖；
 * ② 两个 Notebook profile 完全相同 → 只建一个共享 variant；
 * ③ Notebook A rebuild 不覆盖 Notebook B 的 variant；
 * ⑦ embedding model 改变 → 新 VectorIndexVariant 建立、旧 variant 保留不被覆盖。
 * （④⑤⑥ 属 Phase 2/3，本阶段不覆盖。）
 */

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-variants-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

/** 8 维确定性伪嵌入；模型身份跟随请求里的 modelRef（支持嵌入模型切换场景）。 */
function createManager(lingxiHome: string) {
  const embedCalls: Array<{ modelId: string; texts: string[] }> = [];
  let contextWindowTokens = 8192;
  const manager = new KnowledgeManager({
    lingxiHome,
    getEmbeddingModelContextWindow: () => contextWindowTokens,
    embedTextsForModel: async (request) => {
      embedCalls.push({ modelId: request.modelRef.id, texts: [...request.texts] });
      return {
        vectors: request.texts.map((text) => {
          const vector = new Array(8).fill(0);
          vector[text.length % 8] = (text.length % 7) + 1;
          return vector;
        }),
        dimensions: 8,
        model: { provider: request.modelRef.provider, id: request.modelRef.id, api: "openai", dimensions: 8 },
      };
    },
    canEmbedWithModel: () => true,
  });
  managers.push(manager);
  return { manager, embedCalls, setContextWindowTokens: (tokens: number) => { contextWindowTokens = tokens; } };
}

function modelKeyOf(provider: string, modelId: string): string {
  return crypto.createHash("sha256")
    .update(JSON.stringify([provider, modelId, "openai", 8]), "utf8")
    .digest("hex");
}

/** 六章各四个固定粒度片段；历史配置仍区分变体身份，但不再改变 v3 的片段大小。 */
function novelText(): string {
  const chapters: string[] = [];
  for (let index = 1; index <= 6; index += 1) {
    const paragraph = "末日之后的城市在长夜里延伸，幸存者提着灯穿过废墟。".repeat(64);
    chapters.push(`第${index}章 长夜\n\n${paragraph}`);
  }
  return chapters.join("\n\n");
}

/** 导入 + 解析 + 入队 + 跑完摄入（与路由调用序列一致）。 */
async function ingestInto(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  source: { sourceId: string; artifactId?: string } | { text: string },
) {
  if ("text" in source) {
    const imported = await manager.importPastedText({
      studioId,
      notebookId,
      text: source.text,
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
    return { sourceId: imported.source.id, artifactId: artifact.id };
  }
  manager.enqueueSourceIngestion({
    studioId,
    notebookId,
    sourceId: source.sourceId,
    artifactId: source.artifactId,
  });
  await manager.ingestion.drainQueue();
  return { sourceId: source.sourceId, artifactId: source.artifactId! };
}

/** 该 notebook 当前 profile 下此 artifact 的 ChunkIndexVariant（经惰性绑定解析）。 */
function resolveVariant(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  artifactId: string,
) {
  const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifactId });
  const strategy = resolveKnowledgeChunkerConfig(blocks, {
    targetChars: manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId }),
  }).strategy;
  const { chunkProfile, retrievalProfile } = manager.store.resolveNotebookRetrievalProfile({
    studioId,
    notebookId,
    strategy,
  });
  const variant = manager.indexStore.resolveChunkIndexVariant(artifactId, chunkProfile.profileHash);
  return { chunkProfile, retrievalProfile, variant };
}

function countArtifactVariants(manager: KnowledgeManager, artifactId: string): number {
  return manager.indexStore.db.prepare(`
    SELECT COUNT(*) AS count FROM chunk_index_variants WHERE parse_artifact_id = ?
  `).get(artifactId).count;
}

describe("Profile-aware Index（任务书 §八十九 ①②③⑦）", () => {
  it("① 同一 Source 属于两个 Notebook、chunk 配置不同 → 两个 ChunkIndexVariant 并存互不覆盖", async () => {
    const { manager } = createManager(tempHome());
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "甲（历史配置 5000）" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebookA.id,
      embeddingModelRef: { id: "emb-1", provider: "fake" },
      chunkTargetChars: 5000,
    });
    const notebookB = manager.createNotebook({ studioId, name: "乙（历史配置 300）" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebookB.id,
      embeddingModelRef: { id: "emb-1", provider: "fake" },
      chunkTargetChars: 300,
    });

    const { sourceId, artifactId } = await ingestInto(manager, studioId, notebookA.id, { text: novelText() });
    const variantA = resolveVariant(manager, studioId, notebookA.id, artifactId);
    expect(variantA.variant?.status).toBe("ready");
    const chunksA = manager.indexStore.listVariantChunks(variantA.variant!.id);

    // 共享 Source 加入 Notebook B 并按 B 的配置摄入。
    manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId });
    await ingestInto(manager, studioId, notebookB.id, { sourceId, artifactId });

    const variantB = resolveVariant(manager, studioId, notebookB.id, artifactId);
    expect(variantB.variant?.status).toBe("ready");
    expect(variantB.chunkProfile.profileHash).not.toBe(variantA.chunkProfile.profileHash);
    expect(variantB.variant!.id).not.toBe(variantA.variant!.id);
    expect(countArtifactVariants(manager, artifactId)).toBe(2);

    // 互不覆盖：B 的变体建立后 A 的 chunk 集原样保留。
    const chunksB = manager.indexStore.listVariantChunks(variantB.variant!.id);
    expect(chunksA).toHaveLength(24);
    expect(chunksB).toHaveLength(24);
    expect(chunksB.map(({ text, sectionId, spans }) => ({ text, sectionId, spans })))
      .toEqual(chunksA.map(({ text, sectionId, spans }) => ({ text, sectionId, spans })));
    expect(chunksB.every(chunk => !chunksA.some(original => original.id === chunk.id))).toBe(true);
    const blocks = new Map(manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifactId }).map(block => [block.id, block]));
    for (const chunks of [chunksA, chunksB]) {
      const sectionIds = [...new Set(chunks.map(chunk => chunk.sectionId))];
      expect(sectionIds).toHaveLength(6);
      expect(sectionIds.every(id => typeof id === "string" && id.length > 0)).toBe(true);
      for (const sectionId of sectionIds) {
        const sectionChunks = chunks.filter(chunk => chunk.sectionId === sectionId);
        expect(sectionChunks).toHaveLength(4);
        expect(sectionChunks.slice(0, -1).every(chunk => chunk.tokenCount >= 511)).toBe(true);
      }
      for (const chunk of chunks) {
        expect(chunk.tokenCount).toBe(estimateTextTokens(chunk.text));
        expect(chunk.tokenCount).toBeGreaterThan(0);
        expect(chunk.tokenCount).toBeLessThanOrEqual(512);
        expect(chunk.spans.length).toBeGreaterThan(0);
        for (const span of chunk.spans) {
          expect(chunk.text.slice(span.chunkStartOffset, span.chunkEndOffset))
            .toBe(blocks.get(span.blockId)!.text.slice(span.blockStartOffset, span.blockEndOffset));
        }
      }
    }
    expect(manager.indexStore.listVariantChunks(variantA.variant!.id).map(chunk => chunk.id))
      .toEqual(chunksA.map(chunk => chunk.id));
  });

  it("② 两个 Notebook profile 完全相同 → 只建一个共享 variant", async () => {
    const { manager, embedCalls } = createManager(tempHome());
    const studioId = "studio-a";
    const sameConfig = {
      embeddingModelRef: { id: "emb-1", provider: "fake" },
      chunkTargetChars: 1200,
    };
    const notebookA = manager.createNotebook({ studioId, name: "甲" });
    manager.updateNotebookSettings({ studioId, notebookId: notebookA.id, ...sameConfig });
    const notebookB = manager.createNotebook({ studioId, name: "乙" });
    manager.updateNotebookSettings({ studioId, notebookId: notebookB.id, ...sameConfig });

    const { sourceId, artifactId } = await ingestInto(manager, studioId, notebookA.id, { text: novelText() });
    manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId });
    const embedCallsBeforeB = embedCalls.length;
    await ingestInto(manager, studioId, notebookB.id, { sourceId, artifactId });

    const resolvedA = resolveVariant(manager, studioId, notebookA.id, artifactId);
    const resolvedB = resolveVariant(manager, studioId, notebookB.id, artifactId);
    expect(resolvedA.chunkProfile.profileHash).toBe(resolvedB.chunkProfile.profileHash);
    expect(resolvedA.retrievalProfile.id).toBe(resolvedB.retrievalProfile.id);
    expect(resolvedA.variant!.id).toBe(resolvedB.variant!.id);
    expect(countArtifactVariants(manager, artifactId)).toBe(1);

    // 共享 variant：B 的摄入 chunk/FTS 指纹命中跳过；embed 相位第一批确认模型身份后
    // hasArtifact 命中跳过——只多跑一个探测批（既有契约，见 knowledge-ingestion.test.ts），
    // 向量行不重复写入。
    const civ = knowledgeChunkIndexVariantId(artifactId, resolvedA.chunkProfile.profileHash);
    const viv = knowledgeVectorIndexVariantId(civ, modelKeyOf("fake", "emb-1"));
    const vectorCount = manager.vectorIndex.db.prepare(`
      SELECT COUNT(*) AS count FROM chunk_vectors WHERE vector_index_variant_id = ?
    `).get(viv).count;
    expect(embedCalls.length).toBe(embedCallsBeforeB + 1);
    expect(manager.vectorIndex.db.prepare(`
      SELECT COUNT(*) AS count FROM chunk_vectors WHERE vector_index_variant_id = ?
    `).get(viv).count).toBe(vectorCount);
  });

  it("③ Notebook A rebuild 不覆盖 Notebook B 的 variant", async () => {
    const { manager } = createManager(tempHome());
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "甲" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebookA.id,
      embeddingModelRef: { id: "emb-1", provider: "fake" },
      chunkTargetChars: 5000,
    });
    const notebookB = manager.createNotebook({ studioId, name: "乙" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebookB.id,
      embeddingModelRef: { id: "emb-1", provider: "fake" },
      chunkTargetChars: 300,
    });

    const { sourceId, artifactId } = await ingestInto(manager, studioId, notebookA.id, { text: novelText() });
    manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId });
    await ingestInto(manager, studioId, notebookB.id, { sourceId, artifactId });

    const beforeA = resolveVariant(manager, studioId, notebookA.id, artifactId);
    const beforeB = resolveVariant(manager, studioId, notebookB.id, artifactId);
    const chunksB = manager.indexStore.listVariantChunks(beforeB.variant!.id).map(chunk => chunk.id);
    expect(countArtifactVariants(manager, artifactId)).toBe(2);

    // A 改分块配置 → rebuild：ensure 新 profile 的 variant 后台 build。
    manager.updateNotebookSettings({ studioId, notebookId: notebookA.id, chunkTargetChars: 800 });
    await manager.ingestion.drainQueue();

    const afterA = resolveVariant(manager, studioId, notebookA.id, artifactId);
    expect(afterA.chunkProfile.profileHash).not.toBe(beforeA.chunkProfile.profileHash);
    expect(afterA.variant?.status).toBe("ready");
    // 旧 A 变体保留（不被原地覆盖），B 变体与其 chunk 集原样不动。
    expect(manager.indexStore.resolveChunkIndexVariant(artifactId, beforeA.chunkProfile.profileHash)?.status)
      .toBe("ready");
    expect(countArtifactVariants(manager, artifactId)).toBe(3);
    const afterB = resolveVariant(manager, studioId, notebookB.id, artifactId);
    expect(afterB.variant!.id).toBe(beforeB.variant!.id);
    expect(afterB.variant?.status).toBe("ready");
    expect(manager.indexStore.listVariantChunks(afterB.variant!.id).map(chunk => chunk.id)).toEqual(chunksB);
  });

  it("⑦ embedding model 改变 → 新 VectorIndexVariant 建立、旧 variant 保留不被覆盖", async () => {
    const { manager, embedCalls, setContextWindowTokens } = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: { id: "emb-1", provider: "fake" },
      chunkTargetChars: null,
    });
    const { artifactId } = await ingestInto(manager, studioId, notebook.id, { text: novelText() });
    expect(manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id })).toBe(2048);

    const { chunkProfile } = resolveVariant(manager, studioId, notebook.id, artifactId);
    const civ = knowledgeChunkIndexVariantId(artifactId, chunkProfile.profileHash);
    const chunksBefore = manager.indexStore.listVariantChunks(civ);
    expect(chunksBefore).toHaveLength(24);
    const vivEmb1 = knowledgeVectorIndexVariantId(civ, modelKeyOf("fake", "emb-1"));
    expect(manager.vectorIndex.getVariant(vivEmb1)).toMatchObject({
      chunkIndexVariantId: civ,
      status: "ready",
    });
    const hitsEmb1 = manager.vectorIndex.search({
      vectorIndexVariantIds: [vivEmb1],
      model: { key: modelKeyOf("fake", "emb-1"), provider: "fake", modelId: "emb-1", protocol: "openai", dimensions: 8 },
      queryVector: [1, 0, 0, 0, 0, 0, 0, 0],
      limit: 5,
    });
    expect(hitsEmb1.length).toBeGreaterThan(0);

    // 换更大窗口的嵌入模型：仅建立新向量变体，固定片段与原引用保持不动。
    setContextWindowTokens(128_000);
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: { id: "emb-2", provider: "fake" },
    });
    await manager.ingestion.drainQueue();

    expect(manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id })).toBe(2048);
    expect(manager.indexStore.listVariantChunks(civ)).toEqual(chunksBefore);
    expect(resolveVariant(manager, studioId, notebook.id, artifactId).variant!.id).toBe(civ);
    const secondModelTexts = embedCalls.filter(call => call.modelId === "emb-2").flatMap(call => call.texts);
    expect(secondModelTexts.length).toBeGreaterThan(0);
    expect(secondModelTexts.every(text => estimateTextTokens(text) <= 512)).toBe(true);
    expect(secondModelTexts.every(text => chunksBefore.some(chunk => chunk.text === text))).toBe(true);

    const vivEmb2 = knowledgeVectorIndexVariantId(civ, modelKeyOf("fake", "emb-2"));
    expect(vivEmb2).not.toBe(vivEmb1);
    expect(manager.vectorIndex.getVariant(vivEmb2)).toMatchObject({
      chunkIndexVariantId: civ,
      status: "ready",
    });
    // 旧 variant 保留且向量仍可检索（不被错误覆盖/清除）。
    expect(manager.vectorIndex.getVariant(vivEmb1)?.status).toBe("ready");
    expect(manager.vectorIndex.search({
      vectorIndexVariantIds: [vivEmb1],
      model: { key: modelKeyOf("fake", "emb-1"), provider: "fake", modelId: "emb-1", protocol: "openai", dimensions: 8 },
      queryVector: [1, 0, 0, 0, 0, 0, 0, 0],
      limit: 5,
    }).length).toBeGreaterThan(0);
  });
});
