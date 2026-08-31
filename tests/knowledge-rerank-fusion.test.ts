import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import {
  KNOWLEDGE_EMBEDDING_DEADLINE_MS,
  KNOWLEDGE_RERANK_CLEAR_MARGIN,
  KNOWLEDGE_RERANK_DEADLINE_MS,
  KNOWLEDGE_RRF_K,
} from "../lib/knowledge/knowledge-query-service.ts";
import { KNOWLEDGE_FAST_RERANK_DEADLINE_MS } from "../lib/knowledge/knowledge-context-injector.ts";

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

// ─────────────── rerank 期限降级（2026-08-30 延迟加固） ───────────────

describe("rerank 期限与传输失败降级", () => {
  it("rerank 永不返回：KNOWLEDGE_RERANK_DEADLINE_MS 后降级 RRF 名次并留痕，检索不失败", async () => {
    const hangManager = new KnowledgeManager({
      lingxiHome: tempHome(),
      embedTextsForModel: async (request) => ({
        vectors: fakeEmbedVectors(request.texts),
        dimensions: 8,
        model: { provider: "fake", id: EMB_REF.id, api: "openai", dimensions: 8 },
      }),
      canEmbedWithModel: () => true,
      rerankForModel: () => new Promise(() => {}),
    });
    managers.push(hangManager);
    const studioId = "studio-hang";
    const nb = hangManager.createNotebook({ studioId, name: "甲本" });
    hangManager.updateNotebookSettings({
      studioId,
      notebookId: nb.id,
      embeddingModelRef: EMB_REF,
      rerankModelRef: RERANK_X,
    });
    await ingestPasted(hangManager, studioId, nb.id, TEXT_A1, "甲一.txt");
    await ingestPasted(hangManager, studioId, nb.id, TEXT_A2, "甲二.txt");

    vi.useFakeTimers();
    try {
      const pending = hangManager.queryService.retrieveForNotebooks({
        studioId,
        notebookIds: [nb.id],
        question: QUESTION_B,
      });
      await vi.advanceTimersByTimeAsync(KNOWLEDGE_RERANK_DEADLINE_MS + 10);
      const result = await pending;

      // 检索本体成功：hybrid 通道照常出候选（RRF 名次），不因重排挂死而失败。
      expect(result.retrievalMode).toBe("hybrid");
      expect(result.candidates.length).toBeGreaterThan(0);
      // 显式留痕（禁静默降级）：带笔记本归属。
      expect(result.rerankDegradeReasons).toBeDefined();
      expect(result.rerankDegradeReasons!.join("; ")).toContain("甲本");
      expect(result.rerankDegradeReasons!.join("; ")).toContain("deadline");
    } finally {
      vi.useRealTimers();
    }
  });

  it("rerank 传输类失败（非 KnowledgeError）：同样降级 RRF 名次并留痕，不再炸整个检索", async () => {
    const failManager = new KnowledgeManager({
      lingxiHome: tempHome(),
      embedTextsForModel: async (request) => ({
        vectors: fakeEmbedVectors(request.texts),
        dimensions: 8,
        model: { provider: "fake", id: EMB_REF.id, api: "openai", dimensions: 8 },
      }),
      canEmbedWithModel: () => true,
      rerankForModel: async () => {
        throw new Error("connection reset by peer");
      },
    });
    managers.push(failManager);
    const studioId = "studio-fail";
    const nb = failManager.createNotebook({ studioId, name: "乙本" });
    failManager.updateNotebookSettings({
      studioId,
      notebookId: nb.id,
      embeddingModelRef: EMB_REF,
      rerankModelRef: RERANK_Y,
    });
    await ingestPasted(failManager, studioId, nb.id, TEXT_B1, "乙一.txt");
    await ingestPasted(failManager, studioId, nb.id, TEXT_B2, "乙二.txt");

    const result = await failManager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [nb.id],
      question: QUESTION_B,
    });

    expect(result.retrievalMode).toBe("hybrid");
    expect(result.candidates.length).toBeGreaterThan(0);
    expect(result.rerankDegradeReasons?.join("; ")).toContain("connection reset");
  });
});

// ─────────────── 查询嵌入失败/期限降级（2026-08-30 延迟加固） ───────────────

describe("查询嵌入失败与期限降级", () => {
  type EmbedMode = "ok" | "fail" | "hang" | "invalid" | "abort";
  async function setupEmbedFailNotebook() {
    let embedMode: EmbedMode = "ok";
    const manager = new KnowledgeManager({
      lingxiHome: tempHome(),
      embedTextsForModel: async (request) => {
        if (embedMode === "fail") throw new Error("connection reset by peer");
        if (embedMode === "hang") return new Promise(() => {});
        if (embedMode === "invalid") {
          return { vectors: [], dimensions: 8, model: { provider: "fake", id: EMB_REF.id, api: "openai", dimensions: 8 } };
        }
        if (embedMode === "abort") {
          const error = new Error("The operation was aborted");
          error.name = "AbortError";
          throw error;
        }
        return {
          vectors: fakeEmbedVectors(request.texts),
          dimensions: 8,
          model: { provider: "fake", id: EMB_REF.id, api: "openai", dimensions: 8 },
        };
      },
      canEmbedWithModel: () => true,
    });
    managers.push(manager);
    const studioId = "studio-embed-fail";
    const nb = manager.createNotebook({ studioId, name: "嵌入降级本" });
    manager.updateNotebookSettings({ studioId, notebookId: nb.id, embeddingModelRef: EMB_REF });
    // 摄入期用 "ok" 模式：chunk+FTS+向量全就绪（失败只发生在查询期）。
    await ingestPasted(manager, studioId, nb.id, TEXT_A1, "甲一.txt");
    return { manager, studioId, nb, setMode: (mode: EmbedMode) => { embedMode = mode; } };
  }

  it("embedder 网络错：降级纯 FTS + KNOWLEDGE_EMBEDDING_FAILED 留痕，FTS 候选不丢", async () => {
    const { manager, studioId, nb, setMode } = await setupEmbedFailNotebook();
    setMode("fail");
    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [nb.id],
      question: QUESTION_B,
    });
    expect(result.retrievalMode).toBe("fts");
    expect(result.retrievalModeRequested).toBe("hybrid");
    expect(result.candidates.length).toBeGreaterThan(0);
    expect(result.degraded.some(item => item.reason === "KNOWLEDGE_EMBEDDING_FAILED")).toBe(true);
    expect(result.degraded.find(item => item.reason === "KNOWLEDGE_EMBEDDING_FAILED")?.detail)
      .toContain("connection reset");
  });

  it("embedder 挂起：KNOWLEDGE_EMBEDDING_DEADLINE_MS 后降级 FTS（不再挂满 300s）", async () => {
    const { manager, studioId, nb, setMode } = await setupEmbedFailNotebook();
    setMode("hang");
    vi.useFakeTimers();
    try {
      const pending = manager.queryService.retrieveForNotebooks({
        studioId,
        notebookIds: [nb.id],
        question: QUESTION_B,
      });
      await vi.advanceTimersByTimeAsync(KNOWLEDGE_EMBEDDING_DEADLINE_MS + 10);
      const result = await pending;
      expect(result.retrievalMode).toBe("fts");
      expect(result.candidates.length).toBeGreaterThan(0);
      expect(result.degraded.some(item => item.reason === "KNOWLEDGE_EMBEDDING_FAILED")).toBe(true);
      expect(result.degraded.find(item => item.reason === "KNOWLEDGE_EMBEDDING_FAILED")?.detail)
        .toContain("deadline");
    } finally {
      vi.useRealTimers();
    }
  });

  it("外部 signal 取消：abort 原样上抛（用户取消不降级）", async () => {
    const { manager, studioId, nb, setMode } = await setupEmbedFailNotebook();
    setMode("abort");
    const controller = new AbortController();
    controller.abort();
    await expect(manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [nb.id],
      question: QUESTION_B,
      signal: controller.signal,
    })).rejects.toMatchObject({ name: "AbortError" });
  });

  it("嵌入响应非法（vectors 数量不符）：降级不抛，留痕 EMBEDDING_FAILED", async () => {
    const { manager, studioId, nb, setMode } = await setupEmbedFailNotebook();
    setMode("invalid");
    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [nb.id],
      question: QUESTION_B,
    });
    expect(result.retrievalMode).toBe("fts");
    expect(result.candidates.length).toBeGreaterThan(0);
    expect(result.degraded.some(item => item.reason === "KNOWLEDGE_EMBEDDING_FAILED")).toBe(true);
  });

  it("向量库 search 意外错误：降级 FTS + VECTOR_NOT_READY 留痕", async () => {
    const { manager, studioId, nb } = await setupEmbedFailNotebook();
    vi.spyOn(manager.vectorIndex, "search").mockImplementation(() => {
      throw new Error("sqlite bus error");
    });
    try {
      const result = await manager.queryService.retrieveForNotebooks({
        studioId,
        notebookIds: [nb.id],
        question: QUESTION_B,
      });
      expect(result.retrievalMode).toBe("fts");
      expect(result.candidates.length).toBeGreaterThan(0);
      expect(result.degraded.some(item => item.reason === "KNOWLEDGE_VECTOR_NOT_READY")).toBe(true);
      expect(result.degraded.find(item => item.reason === "KNOWLEDGE_VECTOR_NOT_READY")?.detail)
        .toContain("vector search failed");
    } finally {
      vi.restoreAllMocks();
    }
  });
});

// ─────────────── rerank 动态门控与快速档期限（2026-08-31 两档化） ───────────────

describe("rerank 动态门控与快速档期限", () => {
  // 语料设计（marker 嵌入：甲一→basis(0)、乙二→0.9·basis(0)、其余→basis(1)）：
  // - 清晰场景 Q_CLEAR：只有「甲一」文档 FTS 命中 + 向量 rank-0 → top-1 双通道、
  //   top-2 单通道 → RRF 融合分 margin ≈ 0.017 ≥ 阈值 → 门控跳过重排；
  // - 扎堆场景 Q_BUNCHED：两文档双通道命中且名次紧贴 → margin ≈ 0.0005 → 门控
  //   放行重排。
  const Q_CLEAR = "甲一 独特词";
  const Q_BUNCHED = "哨站 观测 甲一 乙二";

  async function setupMarginNotebook(options?: { hangRerank?: boolean }) {
    const rerankCalls: Array<{ documents: string[] }> = [];
    const manager = new KnowledgeManager({
      lingxiHome: tempHome(),
      embedTextsForModel: async (request) => ({
        vectors: request.texts.map((text) => {
          const vector = new Array(8).fill(0);
          if (text.includes("甲一")) vector[0] = 1;
          else if (text.includes("乙二")) vector[0] = 0.9;
          else vector[1] = 1;
          return vector;
        }),
        dimensions: 8,
        model: { provider: "fake", id: EMB_REF.id, api: "openai", dimensions: 8 },
      }),
      canEmbedWithModel: () => true,
      rerankForModel: options?.hangRerank
        ? () => new Promise(() => {})
        : async (request) => {
          rerankCalls.push({ documents: [...request.documents] });
          // 保持输入序返回（分数单调递减）：只证明「被调用过」，不改名次语义。
          return {
            results: request.documents.map((_, index) => ({ index, score: 0.9 - index * 0.01 })),
          };
        },
    });
    managers.push(manager);
    const studioId = "studio-margin";
    const nb = manager.createNotebook({ studioId, name: "门控本" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: nb.id,
      embeddingModelRef: EMB_REF,
      rerankModelRef: RERANK_X,
    });
    await ingestPasted(manager, studioId, nb.id, "甲一 独特词 甲一哨站的观测记录", "甲一.txt");
    await ingestPasted(manager, studioId, nb.id, "乙二 附近的哨站记录", "乙二.txt");
    await ingestPasted(manager, studioId, nb.id, "丙三 的备用段落", "丙三.txt");
    return { manager, studioId, nb, rerankCalls };
  }

  it("头部清晰（top-1 双通道领先 ≥ 阈值）：门控跳过重排，保持 RRF 名次并留痕", async () => {
    const { manager, studioId, nb, rerankCalls } = await setupMarginNotebook();
    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [nb.id],
      question: Q_CLEAR,
      rerankPolicy: { marginGate: true },
    });
    // 重排零调用（省一次网络往返）——结果直接用 RRF 名次。
    expect(rerankCalls).toHaveLength(0);
    expect(result.retrievalMode).toBe("hybrid");
    expect(result.candidates.length).toBeGreaterThan(0);
    expect(result.candidates[0].text).toContain("甲一");
    // 主动跳过≠降级：独立留痕字段，带笔记本归属与阈值语义。
    expect(result.rerankSkippedReasons).toBeDefined();
    expect(result.rerankSkippedReasons!.join("; ")).toContain("门控本");
    expect(result.rerankSkippedReasons!.join("; ")).toContain("margin gate");
    expect(result.rerankSkippedReasons!.join("; ")).toContain(String(KNOWLEDGE_RERANK_CLEAR_MARGIN));
    expect(result.rerankDegradeReasons).toBeUndefined();
  });

  it("未开门控（详细档缺省）：即使头部清晰也照常重排（既有行为回归锚）", async () => {
    const { manager, studioId, nb, rerankCalls } = await setupMarginNotebook();
    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [nb.id],
      question: Q_CLEAR,
    });
    expect(rerankCalls).toHaveLength(1);
    expect(result.rerankSkippedReasons).toBeUndefined();
    expect(result.rerankDegradeReasons).toBeUndefined();
  });

  it("分数扎堆（双通道名次紧贴）：门控放行重排", async () => {
    const { manager, studioId, nb, rerankCalls } = await setupMarginNotebook();
    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [nb.id],
      question: Q_BUNCHED,
      rerankPolicy: { marginGate: true },
    });
    expect(rerankCalls).toHaveLength(1);
    expect(result.rerankSkippedReasons).toBeUndefined();
    expect(result.candidates.length).toBeGreaterThan(0);
  });

  it("快速档期限收紧：门控放行（扎堆）+ rerank 挂起 → 5s 后降级 RRF 名次并留痕（不等到默认 15s）", async () => {
    const { manager, studioId, nb } = await setupMarginNotebook({ hangRerank: true });
    vi.useFakeTimers();
    try {
      const pending = manager.queryService.retrieveForNotebooks({
        studioId,
        notebookIds: [nb.id],
        question: Q_BUNCHED,
        rerankPolicy: { marginGate: true, deadlineMs: KNOWLEDGE_FAST_RERANK_DEADLINE_MS },
      });
      await vi.advanceTimersByTimeAsync(KNOWLEDGE_FAST_RERANK_DEADLINE_MS + 10);
      const result = await pending;
      // 5s 期限即降级：候选保持 RRF 名次，检索不失败，留痕带收紧后的期限值。
      expect(result.retrievalMode).toBe("hybrid");
      expect(result.candidates.length).toBeGreaterThan(0);
      expect(result.rerankDegradeReasons?.join("; ")).toContain("门控本");
      expect(result.rerankDegradeReasons?.join("; ")).toContain(`${KNOWLEDGE_FAST_RERANK_DEADLINE_MS}ms`);
    } finally {
      vi.useRealTimers();
    }
  });
});
