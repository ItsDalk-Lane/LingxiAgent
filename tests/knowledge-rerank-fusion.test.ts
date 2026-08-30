import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { KNOWLEDGE_RRF_K } from "../lib/knowledge/knowledge-query-service.ts";

/**
 * 任务书 §二十四/§二十五/§九十二：rerank 按笔记本引用真正路由 + 跨笔记本
 * rank-based RRF 融合。融合只消费名次——rerank 分数按各自模型归一、cosine
 * 分数按各自嵌入模型，跨模型/跨笔记本不可比。断言层防两类回归：
 * ① 跨笔记本按 raw score 混排；② rerank 闭包在并行路径下串候选/串模型。
 */
const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-rerank-fusion-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

const EMB_REF = { id: "emb-1", provider: "fake" };
const RERANK_X = { id: "rerank-x", provider: "fake" };
const RERANK_Y = { id: "rerank-y", provider: "fake" };

/** 8 维确定性伪嵌入（与 knowledge-query-config-alignment 同一 scheme）：hybrid 通道用。 */
function fakeEmbedVectors(texts: string[]) {
  return texts.map((text) => {
    const vector = new Array(8).fill(0);
    vector[text.length % 8] = (text.length % 7) + 1;
    return vector;
  });
}

async function ingestPasted(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  text: string,
  displayName: string,
) {
  const imported = await manager.importPastedText({ studioId, notebookId, text, displayName });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  manager.enqueueSourceIngestion({
    studioId,
    notebookId,
    sourceId: imported.source.id,
    artifactId: artifact.id,
  });
  await manager.ingestion.drainQueue();
  return imported;
}

// —— rerank 融合场景（A→X、B→Y，共享源「共档」同时在两个笔记本）——
// 每个 source 都是短文本 = 恰一个 chunk；marker 唯一标识候选。
const QUESTION_B = "星图定位";
const TEXT_SHARED = "星图定位手册：共享的天球坐标基准段（共档）。";
const TEXT_A1 = "星图定位。星图定位。星图定位。甲一哨站的观测记录。";
const TEXT_A2 = "星图定位。星图定位。甲二哨站的观测记录。";
const TEXT_A3 = "星图定位。甲三哨站的观测记录。";
const TEXT_B1 = "星图定位。星图定位。星图定位。乙一哨站的观测记录。";
const TEXT_B2 = "星图定位。星图定位。乙二哨站的观测记录。";
const TEXT_B3 = "星图定位。乙三哨站的观测记录。";

const ALL_MARKERS = ["甲一", "甲二", "甲三", "乙一", "乙二", "乙三", "共档", "东一", "东二", "东三", "西一", "西二", "西三"];

function markerOf(doc: string): string {
  const marker = ALL_MARKERS.find(item => doc.includes(item));
  if (!marker) throw new Error(`test fixture: no marker in doc: ${doc.slice(0, 40)}`);
  return marker;
}

/**
 * fake rerank 按模型分派：X 的分数量级在 [0.9,1.0]、Y 在 [0.1,0.2]（量级刻意
 * 不可比）；返回序即该笔记本的最终名次——A 列 [甲一,甲二,共档,甲三]、
 * B 列 [乙一,共档,乙二,乙三]（「共档」在 A 列名次 2、B 列名次 1）。
 */
function createRerankFusionManager(lingxiHome: string) {
  const rerankCalls: Array<{ modelId: string; documents: string[] }> = [];
  const manager = new KnowledgeManager({
    lingxiHome,
    embedTextsForModel: async (request) => ({
      vectors: fakeEmbedVectors(request.texts),
      dimensions: 8,
      model: { provider: "fake", id: EMB_REF.id, api: "openai", dimensions: 8 },
    }),
    canEmbedWithModel: () => true,
    rerankForModel: async (request) => {
      rerankCalls.push({ modelId: request.modelRef.id, documents: [...request.documents] });
      const isX = request.modelRef.id === RERANK_X.id;
      const order = isX ? ["甲一", "甲二", "共档", "甲三"] : ["乙一", "共档", "乙二", "乙三"];
      return {
        results: order.map((marker, rank) => ({
          index: request.documents.findIndex(doc => doc.includes(marker)),
          score: isX ? 0.99 - rank * 0.01 : 0.19 - rank * 0.01,
        })),
      };
    },
  });
  managers.push(manager);
  return { manager, rerankCalls };
}

/** 甲/乙两笔记本：A 配 X、B 配 Y，共享源「共档」同时挂两个笔记本。 */
async function setupRerankFusionNotebooks() {
  const { manager, rerankCalls } = createRerankFusionManager(tempHome());
  const studioId = "studio-a";
  const notebookA = manager.createNotebook({ studioId, name: "甲本" });
  const notebookB = manager.createNotebook({ studioId, name: "乙本" });
  manager.updateNotebookSettings({
    studioId,
    notebookId: notebookA.id,
    embeddingModelRef: EMB_REF,
    rerankModelRef: RERANK_X,
  });
  manager.updateNotebookSettings({
    studioId,
    notebookId: notebookB.id,
    embeddingModelRef: EMB_REF,
    rerankModelRef: RERANK_Y,
  });
  await ingestPasted(manager, studioId, notebookA.id, TEXT_A1, "甲一.txt");
  await ingestPasted(manager, studioId, notebookA.id, TEXT_A2, "甲二.txt");
  await ingestPasted(manager, studioId, notebookA.id, TEXT_A3, "甲三.txt");
  const shared = await ingestPasted(manager, studioId, notebookA.id, TEXT_SHARED, "共档.txt");
  manager.addSourceToNotebook({ studioId, notebookId: notebookB.id, sourceId: shared.source.id });
  await ingestPasted(manager, studioId, notebookB.id, TEXT_B1, "乙一.txt");
  await ingestPasted(manager, studioId, notebookB.id, TEXT_B2, "乙二.txt");
  await ingestPasted(manager, studioId, notebookB.id, TEXT_B3, "乙三.txt");
  return { manager, rerankCalls, studioId, notebookA, notebookB };
}

describe("Knowledge 跨笔记本 rerank 路由与 rank-based RRF 融合", () => {
  it("两个笔记本的 reranker 按引用独立分派，各自只收到本笔记本的候选", async () => {
    const { manager, rerankCalls, studioId, notebookA, notebookB } = await setupRerankFusionNotebooks();

    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebookA.id, notebookB.id],
      question: QUESTION_B,
    });
    expect(result.retrievalMode).toBe("hybrid");

    // 并行路径下 rerank 闭包按笔记本注入：X/Y 各被独立调用恰一次。
    const xCalls = rerankCalls.filter(call => call.modelId === RERANK_X.id);
    const yCalls = rerankCalls.filter(call => call.modelId === RERANK_Y.id);
    expect(xCalls).toHaveLength(1);
    expect(yCalls).toHaveLength(1);
    // X 只见甲本候选（甲* + 共享源），Y 只见乙本候选（乙* + 共享源）：
    // 不串候选、不串模型。
    expect(new Set(xCalls[0].documents.map(markerOf))).toEqual(new Set(["共档", "甲一", "甲二", "甲三"]));
    expect(new Set(yCalls[0].documents.map(markerOf))).toEqual(new Set(["共档", "乙一", "乙二", "乙三"]));
  });

  it("跨笔记本融合只用名次：rerank 分数量级不可比时顺序由名次决定（RRF 求和让共享源登顶）", async () => {
    const { manager, studioId, notebookA, notebookB } = await setupRerankFusionNotebooks();

    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebookA.id, notebookB.id],
      question: QUESTION_B,
    });

    // A 的 rerank 分数全在 [0.96,0.99]、B 全在 [0.16,0.19]：任何跨笔记本
    // score 混排都会把甲本整体排前（[甲一,甲二,共档,甲三,乙一,...]）。
    // rank-based RRF 下共享源「共档」= 1/63（A 列名次 2）+ 1/62（B 列名次 1），
    // 两列贡献之和超过任何单列名次 0 的贡献（1/61），登顶；其余按名次交错，
    // 并列（1/61 / 1/63 / 1/64 两两同值）按 notebookIds 顺序稳定排序。
    expect(result.candidates.map(chunk => markerOf(chunk.text))).toEqual([
      "共档", "甲一", "乙一", "甲二", "乙二", "甲三", "乙三",
    ]);
    expect(result.candidates.map(chunk => markerOf(chunk.text))).not.toEqual([
      "甲一", "甲二", "共档", "甲三", "乙一", "乙二", "乙三",
    ]);
    // 同 chunk.id 去重：首个出现的序列（notebookIds 顺序）保留归属注解。
    expect(result.candidates).toHaveLength(7);
    expect(result.candidates[0].notebookId).toBe(notebookA.id);
  });

  it("不同嵌入模型的 cosine 分数不跨模型比较：跨笔记本顺序由名次决定", async () => {
    // 两笔记本各 3 候选，FTS 名次与向量名次刻意交叉（实测该分词下 bm25 名次
    // 与词频相反：1×/2×/3× 词频的 FTS 名次为 [东三,东二,东一]）：
    // embA（近平行带，cosine ≈[0.9988,0.99999]）向量名次 [东二,东一,东三]、
    // embB（近正交带，cosine ≈[0.0995,0.2873]）向量名次 [西二,西一,西三]。
    // 任何直接比较 raw score 的实现都会偏离名次序：按 cosine 混排得
    // [东二,东一,东三,...]（且 embA 整体压过 embB），按 bm25 混排得
    // [东三,东二,东一,...]。retrieve 内部 FTS/向量按 RRF（k=60）融合：
    // 东二 = 1/62+1/61、东三 = 1/61+1/63、东一 = 1/63+1/62 → 笔记本内名次
    // [东二,东三,东一]（西本同构）；跨笔记本同样只消费名次，各名次层并列按
    // notebookIds 顺序稳定排序。
    const QUESTION_C = "信标阵列";
    const embedVectorC = (modelId: string, text: string): number[] => {
      if (modelId === "embA") {
        if (text === QUESTION_C) return [1, 0];
        if (text.includes("东二")) return [1, 0.005];
        return text.includes("东一") ? [1, 0.01] : [1, 0.05];
      }
      if (text === QUESTION_C) return [1, 0];
      if (text.includes("西二")) return [0.3, 1];
      return text.includes("西一") ? [0.2, 1] : [0.1, 1];
    };
    const manager = new KnowledgeManager({
      lingxiHome: tempHome(),
      embedTextsForModel: async (request) => ({
        vectors: request.texts.map(text => embedVectorC(request.modelRef.id, text)),
        dimensions: 2,
        model: { provider: "fake", id: request.modelRef.id, api: "openai", dimensions: 2 },
      }),
      canEmbedWithModel: () => true,
    });
    managers.push(manager);
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "东本" });
    const notebookB = manager.createNotebook({ studioId, name: "西本" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebookA.id,
      embeddingModelRef: { id: "embA", provider: "fake" },
    });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebookB.id,
      embeddingModelRef: { id: "embB", provider: "fake" },
    });
    await ingestPasted(manager, studioId, notebookA.id, "信标阵列。信标阵列。信标阵列。东一哨站。", "东一.txt");
    await ingestPasted(manager, studioId, notebookA.id, "信标阵列。信标阵列。东二哨站。", "东二.txt");
    await ingestPasted(manager, studioId, notebookA.id, "信标阵列。东三哨站。", "东三.txt");
    await ingestPasted(manager, studioId, notebookB.id, "信标阵列。信标阵列。信标阵列。西一哨站。", "西一.txt");
    await ingestPasted(manager, studioId, notebookB.id, "信标阵列。信标阵列。西二哨站。", "西二.txt");
    await ingestPasted(manager, studioId, notebookB.id, "信标阵列。西三哨站。", "西三.txt");

    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebookA.id, notebookB.id],
      question: QUESTION_C,
    });
    expect(result.retrievalMode).toBe("hybrid");
    expect(result.candidates.map(chunk => markerOf(chunk.text))).toEqual([
      "东二", "西二", "东三", "西三", "东一", "西一",
    ]);
    // 防回归锚：候选载荷携带的分数是笔记本内 RRF 名次和（FTS 名次 1 + 向量
    // 名次 0，k=60），不是任何通道的 raw score（cosine/bm25 都不出检索核心）。
    expect(result.candidates[0].score).toBeCloseTo(
      1 / (KNOWLEDGE_RRF_K + 1) + 1 / (KNOWLEDGE_RRF_K + 2),
      8,
    );
  });
});
