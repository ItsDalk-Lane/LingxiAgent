import { describe, expect, it, vi } from "vitest";
import {
  KNOWLEDGE_DISTILL_MAX_CONCURRENCY,
  KNOWLEDGE_DISTILL_SYSTEM_PROMPT,
  distillKnowledgeEvidence,
  isRateLimitLikeError,
  isTimeoutLikeError,
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

  it("批预算支持取批时求值：中途变化影响后续批次（engine 实测吞吐校准的执行面）", async () => {
    const manyChunks = Array.from({ length: 16 }, (_, i) => chunk(`c${i}`, "内容".repeat(60)));
    const calls: number[] = []; // 每次调用时使用的预算
    let callsMade = 0;
    const distillModel = vi.fn(async ({ batch }: { batch: string }) => {
      callsMade += 1;
      return `提炼:${batch.slice(0, 4)}`;
    });
    // 前 3 次取批用大预算 600（一批装多块），之后降到 120（每批 1-2 块）→
    // 总批数应多于全程 600 的 1-2 批。
    let budgetReads = 0;
    const budget = () => {
      budgetReads += 1;
      return budgetReads <= 3 ? 600 : 120;
    };
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: manyChunks,
      headerOf,
      budgetTokens: 8000,
      batchBudgetTokens: budget,
      distillModel,
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.batches).toBeGreaterThan(3); // 预算收缩 → 批数增加
      expect(callsMade).toBe(result.batches);
    }
  });

  it("onProgress 每批完成即回调（单调递增至总批数）", async () => {
    const manyChunks = Array.from({ length: 8 }, (_, i) => chunk(`c${i}`, "内容".repeat(60)));
    const progress: number[] = [];
    const distillModel = vi.fn(async ({ batch }: { batch: string }) => `提炼:${batch.slice(0, 4)}`);
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: manyChunks,
      headerOf,
      budgetTokens: 4000,
      batchBudgetTokens: 150,
      onProgress: (done) => progress.push(done),
      distillModel,
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(progress.length).toBe(result.batches);
      expect(progress[progress.length - 1]).toBe(result.batches);
      expect(Math.max(...progress)).toBe(result.batches);
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

  it("批预算独立于注入预算：小批预算切出更多批，注入预算只管输出整合", async () => {
    // 事故（2026-08-29）：批预算复用注入预算 → 49.5 万 token/批的巨型请求。
    const manyChunks = Array.from({ length: 12 }, (_, i) => chunk(`c${i}`, "内容".repeat(60)));
    const distillModel = vi.fn(async ({ batch }: { batch: string }) => `提炼:${batch.slice(0, 4)}`);
    // 注入预算 4000（输出整合）；批预算 200（每批约 1-2 块 → 12 块切出多批）。
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: manyChunks,
      headerOf,
      budgetTokens: 4000,
      batchBudgetTokens: 200,
      distillModel,
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.batches).toBeGreaterThan(4); // 小批预算生效（单批预算下只会是 1 批）
      expect(result.sections).toHaveLength(result.batches);
      // sections 按批原始顺序编号（与并行完成顺序无关）。
      expect(result.sections[0].header).toContain("[K1] distilled from evidence blocks K1–");
      expect(result.sections[result.sections.length - 1].header)
        .toContain(`[K${result.batches}] distilled from evidence blocks K`);
    }
  });
});

describe("批间并行与限流自适应降速", () => {
  const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  it("多批并行执行：并发峰值 > 1（串行改并行的直接验证）", async () => {
    const manyChunks = Array.from({ length: 8 }, (_, i) => chunk(`c${i}`, "内容".repeat(60)));
    let active = 0;
    let peak = 0;
    const distillModel = vi.fn(async ({ batch }: { batch: string }) => {
      active += 1;
      peak = Math.max(peak, active);
      await delay(10);
      active -= 1;
      return `提炼:${batch.slice(0, 4)}`;
    });
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: manyChunks,
      headerOf,
      budgetTokens: 4000,
      batchBudgetTokens: 150,
      distillModel,
    });
    expect(result.ok).toBe(true);
    expect(peak).toBeGreaterThan(1);
    expect(peak).toBeLessThanOrEqual(KNOWLEDGE_DISTILL_MAX_CONCURRENCY);
  });

  it("供应商限流：并发逐层减半并重排该批，最终全部成功", async () => {
    const manyChunks = Array.from({ length: 8 }, (_, i) => chunk(`c${i}`, "内容".repeat(60)));
    let active = 0;
    let peak = 0;
    let rateLimited = 0;
    const distillModel = vi.fn(async ({ batch }: { batch: string }) => {
      active += 1;
      peak = Math.max(peak, active);
      try {
        await delay(10);
        // 供应商在 >2 路并发时限流：抛 429 形态错误。
        if (active > 2) {
          rateLimited += 1;
          throw Object.assign(new Error("429 Too Many Requests: rate limit exceeded"), { statusCode: 429 });
        }
        return `提炼:${batch.slice(0, 4)}`;
      } finally {
        active -= 1;
      }
    });
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: manyChunks,
      headerOf,
      budgetTokens: 4000,
      batchBudgetTokens: 150,
      distillModel,
    });
    expect(result.ok).toBe(true);
    expect(rateLimited).toBeGreaterThan(0); // 确实触发过限流并降速恢复
    expect(peak).toBeGreaterThan(2); // 起始按最大路数起跑
    if (result.ok) expect(result.sections).toHaveLength(result.batches);
  });

  it("持续限流超过单批重试上限：整体判失败并携带限流原因", async () => {
    const manyChunks = Array.from({ length: 4 }, (_, i) => chunk(`c${i}`, "内容".repeat(60)));
    const distillModel = vi.fn(async () => {
      throw Object.assign(new Error("rate limit: 429"), { statusCode: 429 });
    });
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: manyChunks,
      headerOf,
      budgetTokens: 4000,
      batchBudgetTokens: 150,
      distillModel,
    });
    expect(result.ok).toBe(false);
    if (result.ok === false) {
      expect(result.reason).toContain("rate-limited");
    }
  });

  it("供应商超时（服务端排队饿死）：同样进减半梯子，降并发后全部成功", async () => {
    // 2026-08-29 实测形态：8 路并发 6 万 token 批全部 90+ 秒零回包（无 429）。
    const manyChunks = Array.from({ length: 8 }, (_, i) => chunk(`c${i}`, "内容".repeat(60)));
    let active = 0;
    let timedOut = 0;
    const distillModel = vi.fn(async ({ batch }: { batch: string }) => {
      active += 1;
      try {
        await delay(10);
        // 供应商在 >2 路并发时排队饿死：抛 LLM_TIMEOUT 形态错误（无 429）。
        if (active > 2) {
          timedOut += 1;
          throw Object.assign(new Error("LLM_TIMEOUT: request timed out"), { code: "LLM_TIMEOUT" });
        }
        return `提炼:${batch.slice(0, 4)}`;
      } finally {
        active -= 1;
      }
    });
    const result = await distillKnowledgeEvidence({
      question: "问题",
      chunks: manyChunks,
      headerOf,
      budgetTokens: 4000,
      batchBudgetTokens: 150,
      distillModel,
    });
    expect(result.ok).toBe(true);
    expect(timedOut).toBeGreaterThan(0); // 超时确实触发过降速
    if (result.ok) expect(result.sections).toHaveLength(result.batches);
  });

  it("isTimeoutLikeError：LLM_TIMEOUT/TimeoutError/文案识别，普通错误不误判", () => {
    expect(isTimeoutLikeError(Object.assign(new Error("x"), { code: "LLM_TIMEOUT" }))).toBe(true);
    expect(isTimeoutLikeError(Object.assign(new Error("x"), { name: "TimeoutError" }))).toBe(true);
    expect(isTimeoutLikeError(new Error("request timed out after 90000ms"))).toBe(true);
    expect(isTimeoutLikeError(new Error("network down"))).toBe(false);
  });

  it("isRateLimitLikeError：429 状态与限流文案识别，普通错误不误判", () => {
    expect(isRateLimitLikeError(Object.assign(new Error("x"), { statusCode: 429 }))).toBe(true);
    expect(isRateLimitLikeError(Object.assign(new Error("x"), { status: 429 }))).toBe(true);
    expect(isRateLimitLikeError(new Error("Too Many Requests"))).toBe(true);
    expect(isRateLimitLikeError(new Error("Quota Exceeded for today"))).toBe(true);
    expect(isRateLimitLikeError(new Error("network down"))).toBe(false);
    expect(isRateLimitLikeError(new Error("1429 chars truncated"))).toBe(false);
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
