import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import {
  buildKnowledgeContextInjection,
  KNOWLEDGE_FAST_MAX_EVIDENCE_ENTRIES,
} from "../lib/knowledge/legacy/legacy-knowledge-context-injector.ts";
import type { KnowledgeReferenceMode } from "../shared/knowledge-refs.ts";

/**
 * 检索质量 golden set（2026-08-31 两档化配套）：固定的「问题 → 应命中源」
 * 语料 + recall 断言，防止提速改动（快速档封顶 / rerank 门控 / 跳拆解）悄悄
 * 回退召回。嵌入用按主题词的确定性伪嵌入（查询与文档共享主题词 → 双通道
 * 命中），走真实 KnowledgeManager 摄入 + FTS/向量/RRF/rerank 全栈。
 *
 * 语义级召回（同义改写、跨语言）不在本门禁范围——那需要真实嵌入模型；
 * 这里守的是「词面清晰的问题必须命中应命中的源」这条底线。
 */
const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-golden-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

const EMB_REF = { id: "emb-golden", provider: "fake" };

/** 主题词 → 8 维基向量首查映射（查询与文档共享主题词即向量命中）。 */
const TOPIC_BASIS: Array<[string, number]> = [
  ["年假", 0],
  ["报销", 1],
  ["remote", 2],
  ["远程", 2],
  ["发布", 3],
  ["值班", 4],
  ["告警", 4],
];

function topicVector(text: string): number[] {
  const vector = new Array(8).fill(0);
  for (const [topic, axis] of TOPIC_BASIS) {
    if (text.toLowerCase().includes(topic)) {
      vector[axis] = 1;
      return vector;
    }
  }
  vector[7] = 1;
  return vector;
}

const CORPUS: Array<{ notebook: "制度本" | "产品本"; name: string; text: string }> = [
  {
    notebook: "制度本",
    name: "年假政策.txt",
    text: "全职员工每年享有 15 天带薪年假。年假申请需提前三个工作日在系统提交，未休完的年假可结转至次年一季度。",
  },
  {
    notebook: "制度本",
    name: "报销流程.txt",
    text: "差旅报销需在行程结束后 30 天内提交发票与行程单。报销审批由直属主管与财务两级完成，单笔超过 5000 元需附加说明。",
  },
  {
    notebook: "制度本",
    name: "remote-work.txt",
    text: "Remote work policy: employees may work remotely up to two days per week. Remote days must be approved by the direct manager in advance.",
  },
  {
    notebook: "产品本",
    name: "产品发布节奏.txt",
    text: "产品每四周发布一个版本。发布前三天进入代码冻结期，只允许修复线上问题的补丁合入。",
  },
  {
    notebook: "产品本",
    name: "值班表.txt",
    text: "团队实行工作日值班制。值班同学负责响应线上告警，收到告警后 15 分钟内需要确认并记录处理进展。",
  },
];

/** golden 问题集：问题 → 必须命中的源名（fast top-12 内全部出现）。 */
const GOLDEN: Array<{ question: string; expectSources: string[] }> = [
  { question: "年假有多少天", expectSources: ["年假政策.txt"] },
  { question: "差旅报销的截止期限", expectSources: ["报销流程.txt"] },
  { question: "远程办公每周可以几天", expectSources: ["remote-work.txt"] },
  { question: "多久发布一个版本", expectSources: ["产品发布节奏.txt"] },
  { question: "线上告警谁来响应", expectSources: ["值班表.txt"] },
  { question: "远程办公和值班制度分别是什么", expectSources: ["remote-work.txt", "值班表.txt"] },
];

async function setupGoldenManager() {
  const manager = new KnowledgeManager({
    lingxiHome: tempHome(),
    embedTextsForModel: async (request) => ({
      vectors: request.texts.map(topicVector),
      dimensions: 8,
      model: { provider: "fake", id: EMB_REF.id, api: "openai", dimensions: 8 },
    }),
    canEmbedWithModel: () => true,
  });
  managers.push(manager);
  const studioId = "studio-golden";
  const notebookByName = new Map<string, string>();
  for (const notebookName of ["制度本", "产品本"]) {
    const notebookId = manager.createNotebook({ studioId, name: notebookName }).id;
    // hybrid 双通道的前提：笔记本显式配置嵌入模型引用（与 rerank 融合测试同姿势）。
    manager.updateNotebookSettings({ studioId, notebookId, embeddingModelRef: EMB_REF });
    notebookByName.set(notebookName, notebookId);
  }
  for (const doc of CORPUS) {
    const imported = await manager.importPastedText({
      studioId,
      notebookId: notebookByName.get(doc.notebook)!,
      text: doc.text,
      displayName: doc.name,
    });
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    manager.enqueueSourceIngestion({
      studioId,
      notebookId: notebookByName.get(doc.notebook)!,
      sourceId: imported.source.id,
      artifactId: artifact.id,
    });
  }
  await manager.ingestion.drainQueue();
  const notebookIds = [...notebookByName.values()];
  return { manager, studioId, notebookIds };
}

async function recallFor(
  manager: KnowledgeManager,
  studioId: string,
  notebookIds: string[],
  question: string,
  mode: KnowledgeReferenceMode,
) {
  const { stats } = await buildKnowledgeContextInjection({
    question,
    mode,
    budgetTokens: 100_000,
    deps: {
      decomposeModel: null,
      retrieve: ({ query }) => manager.queryService.retrieveForNotebooks({ studioId, notebookIds, question: query }),
    },
  });
  return { stats, hitSources: new Set((stats.results ?? []).map(entry => entry.sourceName)) };
}

describe("检索质量 golden set（快速/详细两档 recall 门禁）", () => {
  it("快速档：全部 golden 问题的应命中源都在注入证据内（top-12 封顶不丢必命中源）", async () => {
    const { manager, studioId, notebookIds } = await setupGoldenManager();
    for (const { question, expectSources } of GOLDEN) {
      const { stats, hitSources } = await recallFor(manager, studioId, notebookIds, question, "fast");
      expect(stats.injectedChunks, question).toBeLessThanOrEqual(KNOWLEDGE_FAST_MAX_EVIDENCE_ENTRIES);
      for (const source of expectSources) {
        expect(hitSources.has(source), `fast 档「${question}」应命中 ${source}，实际命中 [${[...hitSources].join(", ")}]`).toBe(true);
      }
      // 分段计时（2026-08-31 观测补齐）：全档携带；快速档 planner 为 0（零拆解）。
      expect(stats.stageTimings, question).toBeDefined();
      expect(stats.stageTimings!.totalMs, question).toBeGreaterThanOrEqual(0);
      expect(stats.stageTimings!.plannerMs, question).toBe(0);
    }
  });

  it("详细档：同一 golden 集 recall 不回退（全量路径回归锚）", async () => {
    const { manager, studioId, notebookIds } = await setupGoldenManager();
    for (const { question, expectSources } of GOLDEN) {
      const { hitSources } = await recallFor(manager, studioId, notebookIds, question, "detailed");
      for (const source of expectSources) {
        expect(hitSources.has(source), `detailed 档「${question}」应命中 ${source}`).toBe(true);
      }
    }
  });
});
