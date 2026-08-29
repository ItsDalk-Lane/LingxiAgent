import { describe, expect, it, vi } from "vitest";
import {
  KNOWLEDGE_DISTILL_SYSTEM_PROMPT,
  distillKnowledgeEvidence,
  planDistillBatches,
} from "../lib/knowledge/knowledge-distiller.ts";
import type { NotebookRetrievalChunk } from "../lib/knowledge/knowledge-query-service.ts";

function chunk(id: string, text: string, notebookName = "资料"): NotebookRetrievalChunk {
  return {
    id,
    parseArtifactId: "parse-1",
    ordinal: 0,
    text,
    tokenCount: 0,
    spans: [],
    score: 1,
    notebookId: "nb-1",
    notebookName,
    sourceId: `src-${id}`,
    sourceName: `源-${id}`,
    headingPath: null,
    pageNumber: null,
  } as NotebookRetrievalChunk;
}

const headerOf = (c: NotebookRetrievalChunk, index: number) => `[K${index + 1}] ${c.sourceName}`;

describe("planDistillBatches（贪心分批）", () => {
  it("预算内全部进一批；超预算切批", () => {
    const small = [chunk("a", "短"), chunk("b", "短")];
    expect(planDistillBatches({ chunks: small, headerOf, budgetTokens: 100 })).toHaveLength(1);

    // 每块中文 10 字 ≈ 11 tokens；预算 12 → 每批一块
    const big = [chunk("a", "一二三四五六七八九十"), chunk("b", "一二三四五六七八九十"), chunk("c", "一二三四五六七八九十")];
    const batches = planDistillBatches({ chunks: big, headerOf, budgetTokens: 12 });
    expect(batches).toHaveLength(3);
    expect(batches.every(batch => batch.chunks.length === 1)).toBe(true);
  });

  it("单块超批预算独占一批（照送不丢）", () => {
    const big = [chunk("a", "巨".repeat(200))];
    const batches = planDistillBatches({ chunks: big, headerOf, budgetTokens: 10 });
    expect(batches).toHaveLength(1);
    expect(batches[0].chunks).toHaveLength(1);
  });

  it("空输入返回空数组", () => {
    expect(planDistillBatches({ chunks: [], headerOf, budgetTokens: 100 })).toEqual([]);
  });
});

describe("distillKnowledgeEvidence（分段压缩编排）", () => {
  const bigChunks = Array.from({ length: 6 }, (_, i) => chunk(`c${i}`, "内容".repeat(60)));

  it("成功路径：每批一次调用、sections 编号延续、批数正确", async () => {
    const distillModel = vi.fn(async ({ batch }: { batch: string }) => `提炼：${batch.slice(0, 5)}`);
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: bigChunks,
      headerOf,
      budgetTokens: 400,
      distillModel,
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.batches).toBeGreaterThan(1);
      expect(result.sections).toHaveLength(result.batches);
      expect(result.sections[0].header).toContain("[K1] distilled from evidence blocks K1–");
      expect(result.sections[0].firstChunk.id).toBe("c0");
    }
  });

  it("首次空输出触发一次纠错重试（correction 携带错误）", async () => {
    // 单批场景（小输入 + 大预算）：重试计数只受本批影响。
    const smallChunks = [chunk("a", "短内容"), chunk("b", "短内容")];
    const calls: any[] = [];
    const distillModel = vi.fn(async (input: any) => {
      calls.push(input);
      return calls.length === 1 ? "" : "有效提炼";
    });
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: smallChunks,
      headerOf,
      budgetTokens: 400,
      distillModel,
    });
    expect(result.ok).toBe(true);
    expect(distillModel).toHaveBeenCalledTimes(2);
    expect(calls[1].correction).toMatchObject({ error: "empty output" });
  });

  it("两次空输出整体判失败（显式原因）", async () => {
    const distillModel = vi.fn(async () => "");
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: bigChunks,
      headerOf,
      budgetTokens: 400,
      distillModel,
    });
    expect(result.ok).toBe(false);
    if (result.ok === false) {
      expect(result.reason).toContain("empty output after one correction retry");
    }
  });

  it("模型抛错整体判失败并携带原因", async () => {
    const distillModel = vi.fn(async () => {
      throw new Error("network down");
    });
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: bigChunks,
      headerOf,
      budgetTokens: 400,
      distillModel,
    });
    expect(result.ok).toBe(false);
    if (result.ok === false) {
      expect(result.reason).toContain("network down");
    }
  });

  it("单批输出超上限被截断（防失控输出挤占整合预算）", async () => {
    // 单批（预算 400 → maxOutput 400 tokens ≈ 363 个中文字）：
    // 万字输出被截到上限内，整合不超预算、不整体失败。
    const smallChunks = [chunk("a", "短内容")];
    const distillModel = vi.fn(async () => "很长".repeat(5000));
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: smallChunks,
      headerOf,
      budgetTokens: 400,
      distillModel,
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      for (const section of result.sections) {
        expect(section.body.length).toBeLessThan(500);
      }
    }
  });

  it("无证据判失败", async () => {
    const distillModel = vi.fn(async () => "x");
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: [],
      headerOf,
      budgetTokens: 60,
      distillModel,
    });
    expect(result.ok).toBe(false);
    expect(distillModel).not.toHaveBeenCalled();
  });
});

describe("KNOWLEDGE_DISTILL_SYSTEM_PROMPT（注入防御与保真规则）", () => {
  it("包含相关性提取、逐字保真、溯源行、不可信数据防御与无围栏规则", () => {
    expect(KNOWLEDGE_DISTILL_SYSTEM_PROMPT).toContain("ONLY the content relevant");
    expect(KNOWLEDGE_DISTILL_SYSTEM_PROMPT).toContain("verbatim");
    expect(KNOWLEDGE_DISTILL_SYSTEM_PROMPT).toContain("sourceId");
    expect(KNOWLEDGE_DISTILL_SYSTEM_PROMPT).toContain("untrusted source data");
    expect(KNOWLEDGE_DISTILL_SYSTEM_PROMPT).toContain("no Markdown fences");
  });
});
