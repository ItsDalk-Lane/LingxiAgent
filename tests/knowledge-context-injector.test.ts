import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  buildKnowledgeContextInjection,
  KNOWLEDGE_INJECTION_FALLBACK_BUDGET_TOKENS,
  resolveKnowledgeInjectionBudgetTokens,
  decomposeQuestion,
  fuseSubQueryResults,
  knowledgeModeGuidance,
  KNOWLEDGE_DECOMPOSE_SYSTEM_PROMPT,
  parseQuestionDecomposition,
  type DecomposeModel,
} from "../lib/knowledge/knowledge-context-injector.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import type { RetrieveForNotebooksResult } from "../lib/knowledge/knowledge-query-service.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-injector-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function validOutput(subQueries: string[] = ["苹果 交付日期", "apple delivery date"]) {
  return JSON.stringify({ intent: "factual", subQueries });
}

function fakeChunk(overrides: Partial<RetrieveForNotebooksResult["candidates"][number]> = {}) {
  return {
    id: `chunk_${Math.random().toString(36).slice(2)}`,
    parseArtifactId: "parse-1",
    ordinal: 0,
    text: "证据文本",
    tokenCount: 4,
    spans: [],
    score: 1,
    notebookId: "nb-1",
    notebookName: "资料",
    sourceId: "src-1",
    sourceName: "源",
    headingPath: null,
    pageNumber: null,
    ...overrides,
  } as RetrieveForNotebooksResult["candidates"][number];
}

function fakeRetrieval(candidates: RetrieveForNotebooksResult["candidates"]): RetrieveForNotebooksResult {
  return { candidates, sources: [], retrievalMode: "fts" };
}

describe("动态注入预算 resolveKnowledgeInjectionBudgetTokens", () => {
  it("常规窗口：窗口 − maxOutput", () => {
    expect(resolveKnowledgeInjectionBudgetTokens({ contextWindow: 128_000, maxTokens: 8_192 })).toBe(119_808);
    expect(resolveKnowledgeInjectionBudgetTokens({ contextWindow: 32_000, maxOutput: 4_096 })).toBe(27_904);
  });

  it("maxOutput 缺失按窗口 25% 预留", () => {
    expect(resolveKnowledgeInjectionBudgetTokens({ contextWindow: 128_000 })).toBe(96_000);
  });

  it("窗口未知/非法回退固定兜底", () => {
    expect(resolveKnowledgeInjectionBudgetTokens(null)).toBe(KNOWLEDGE_INJECTION_FALLBACK_BUDGET_TOKENS);
    expect(resolveKnowledgeInjectionBudgetTokens({})).toBe(KNOWLEDGE_INJECTION_FALLBACK_BUDGET_TOKENS);
    expect(resolveKnowledgeInjectionBudgetTokens({ contextWindow: 0 })).toBe(KNOWLEDGE_INJECTION_FALLBACK_BUDGET_TOKENS);
    expect(resolveKnowledgeInjectionBudgetTokens({ contextWindow: "big" })).toBe(KNOWLEDGE_INJECTION_FALLBACK_BUDGET_TOKENS);
  });

  it("极小窗口夹在预算下限 1000", () => {
    expect(resolveKnowledgeInjectionBudgetTokens({ contextWindow: 2_000, maxTokens: 1_900 })).toBe(1000);
  });
});

describe("拆解输出严格校验", () => {
  it("接受合法 JSON 并保留 intent 与子查询", () => {
    const parsed = parseQuestionDecomposition(validOutput(["q1", "q2"]));
    expect(parsed.intent).toBe("factual");
    expect(parsed.subQueries).toEqual(["q1", "q2"]);
  });

  it("拒绝非 JSON / 非对象 / 字段不符", () => {
    for (const raw of ["not json", "[]", '{"intent":"factual"}', '{"intent":"factual","subQueries":[],"extra":1}']) {
      expect(() => parseQuestionDecomposition(raw)).toThrowError(
        expect.objectContaining({ code: "KNOWLEDGE_MODEL_OUTPUT_INVALID" }),
      );
    }
  });

  it("拒绝非法 intent 枚举与超出 1-4 条的子查询", () => {
    expect(() => parseQuestionDecomposition('{"intent":"other","subQueries":["q"]}')).toThrow();
    expect(() => parseQuestionDecomposition('{"intent":"list","subQueries":[]}')).toThrow();
    expect(() => parseQuestionDecomposition('{"intent":"list","subQueries":["a","b","c","d","e"]}')).toThrow();
  });

  it("拒绝空串与超长子查询", () => {
    expect(() => parseQuestionDecomposition('{"intent":"list","subQueries":["  "]}')).toThrow();
    expect(() => parseQuestionDecomposition(
      JSON.stringify({ intent: "list", subQueries: ["x".repeat(501)] }),
    )).toThrow();
  });

  it("系统提示词携带 schema、围栏禁令与专有名词规则", () => {
    expect(KNOWLEDGE_DECOMPOSE_SYSTEM_PROMPT).toContain('"intent":"factual|summarize|compare|list|reasoning"');
    expect(KNOWLEDGE_DECOMPOSE_SYSTEM_PROMPT).toContain("Do not use Markdown fences");
    expect(KNOWLEDGE_DECOMPOSE_SYSTEM_PROMPT).toContain("exactly as written");
  });
});

describe("decomposeQuestion 纠错与降级", () => {
  it("首次输出合法则直接采用", async () => {
    const calls: any[] = [];
    const callModel: DecomposeModel = async (input) => {
      calls.push(input);
      return validOutput(["子查询一"]);
    };
    const result = await decomposeQuestion({ question: "问题", callModel });
    expect(result.degraded).toBe(false);
    expect(result.subQueries).toEqual(["子查询一"]);
    expect(calls).toHaveLength(1);
    expect(calls[0].correction).toBeUndefined();
  });

  it("首次无效只纠错重试一次；第二次合法则采用", async () => {
    const calls: any[] = [];
    const callModel: DecomposeModel = async (input) => {
      calls.push(input);
      return input.correction ? validOutput(["修正后子查询"]) : "{invalid";
    };
    const result = await decomposeQuestion({ question: "问题", callModel });
    expect(result.degraded).toBe(false);
    expect(result.subQueries).toEqual(["修正后子查询"]);
    expect(calls).toHaveLength(2);
    expect(calls[1].correction).toMatchObject({ previousOutput: "{invalid" });
  });

  it("连续无效降级为原问题单查询并留痕", async () => {
    const callModel = vi.fn(async () => "still not json");
    const result = await decomposeQuestion({ question: "原始问题", callModel });
    expect(result.degraded).toBe(true);
    expect(result.subQueries).toEqual(["原始问题"]);
    expect(result.degradeReason).toBe("model output invalid after one correction retry");
    expect(callModel).toHaveBeenCalledTimes(2);
  });

  it("模型调用抛错（如超时）降级并携带原因", async () => {
    const callModel = vi.fn(async () => {
      throw new Error("timeout after 15000ms");
    });
    const result = await decomposeQuestion({ question: "原始问题", callModel });
    expect(result.degraded).toBe(true);
    expect(result.degradeReason).toBe("model call failed");
    expect(result.degradeDetail).toContain("timeout");
  });

  it("槽位未配置（callModel 为 null）直接单查询并留痕", async () => {
    const result = await decomposeQuestion({ question: "原始问题", callModel: null });
    expect(result.degraded).toBe(true);
    expect(result.degradeReason).toBe("knowledge model slot not configured");
    expect(result.subQueries).toEqual(["原始问题"]);
  });
});

describe("跨子查询融合", () => {
  it("多子查询同时命中的 chunk 排在只命中一次的前面", () => {
    const shared = fakeChunk({ id: "shared", text: "共享" });
    const onlyFirst = fakeChunk({ id: "only-first", text: "仅一次" });
    const resultA = fakeRetrieval([shared, onlyFirst]);
    const resultB = fakeRetrieval([fakeChunk({ id: "other", text: "另一次" }), shared]);
    const fused = fuseSubQueryResults([resultA, resultB]);
    expect(fused[0].id).toBe("shared");
    expect(new Set(fused.map(chunk => chunk.id))).toEqual(new Set(["shared", "only-first", "other"]));
  });
});

describe("注入块生成（纯函数部分）", () => {
  it("问答模式指引带 {{cite:N}} 证据规则，辅助模式不带", () => {
    expect(knowledgeModeGuidance("qa")).toContain("{{cite:N}}");
    expect(knowledgeModeGuidance("qa")).toContain("say so plainly");
    expect(knowledgeModeGuidance("assist")).not.toContain("{{cite:N}}");
    expect(knowledgeModeGuidance("assist")).toContain("general knowledge");
  });

  it("未超预算：全部证据块注入并带定位头；stats 完整记录检索量", async () => {
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      deps: {
        decomposeModel: null,
        distillModel: null,
        retrieve: async () => fakeRetrieval([
          fakeChunk({ text: "证据 A", notebookName: "研究", sourceName: "论文", ordinal: 3, headingPath: ["Intro", "Scope"] }),
          fakeChunk({ text: "证据 B", sourceName: "报告", pageNumber: 12 }),
        ]),
      },
    });
    expect(block).toContain("[KnowledgeContext]");
    expect(block).toContain("[question decomposition unavailable: knowledge model slot not configured]");
    expect(block).toContain('[K1] notebook "研究" / source "论文" (sourceId: src-1) / chunk ordinal 4 / heading: Intro > Scope');
    expect(block).toContain("证据 A");
    expect(block).toContain("page: 12");
    expect(block).not.toContain("Shard manifest");
    expect(block.endsWith("[/KnowledgeContext]")).toBe(true);
    // stats：拆解降级单查询 + 两条候选全部注入，fts 模式，未超预算。
    expect(stats).toMatchObject({
      mode: "qa",
      retrievalMode: "fts",
      subQueries: ["问题"],
      subQueryHits: [2],
      degraded: true,
      degradeReason: "knowledge model slot not configured",
      fusedChunks: 2,
      injectedChunks: 2,
      truncated: false,
      budgetTokens: 6000,
    });
    expect(stats.usedTokens).toBeGreaterThan(0);
    expect(stats.unavailableReason).toBeUndefined();
    // results：ordinal 与注入块 [KN] 编号一致，chunkOrdinal 为源内 1-based，
    // firstLine 为块正文首行（此处均为单行短文本，不触发截断）。
    expect(stats.results).toEqual([
      { ordinal: 1, sourceName: "论文", chunkOrdinal: 4, firstLine: "证据 A" },
      { ordinal: 2, sourceName: "报告", chunkOrdinal: 1, firstLine: "证据 B" },
    ]);
  });

  it("超预算：截断说明 + 分片清单 + 子 Agent 指引；stats 标记 truncated", async () => {
    const candidates = Array.from({ length: 10 }, (_, index) => (
      fakeChunk({ id: `c${index}`, ordinal: index, text: `${"证据".repeat(200)}-${index}` })
    ));
    // 每块正文 400 个中文字符，语言感知口径约 440 tokens/块（旧 chars/4 口径
    // 会低估到 100）；预算 1000 装得下前两块、装不下全部十块。
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "assist",
      budgetTokens: 1000,
      deps: {
        decomposeModel: null,
        distillModel: null,
        retrieve: async () => ({
          candidates,
          sources: [{
            notebookId: "nb-1",
            notebookName: "研究",
            sourceId: "src-1",
            sourceName: "论文",
            parseArtifactId: "parse-1",
            chunkCount: 10,
            firstHeadingPath: ["Intro"],
          }],
          retrievalMode: "fts",
        }),
      },
    });
    expect(block).toContain("omitted to fit the context budget");
    expect(block).toContain("Shard manifest");
    expect(block).toContain('- source "论文" (sourceId: src-1, notebook "研究"): 10 chunks, ordinals 1-10, first heading: Intro');
    expect(block).toContain("`subagent`");
    expect(block).toContain("`knowledge_read`");
    // 截断后注入的块数应少于候选总数。
    const injectedCount = (block.match(/\[K\d+\] notebook/g) || []).length;
    expect(injectedCount).toBeGreaterThan(0);
    expect(injectedCount).toBeLessThan(10);
    expect(stats).toMatchObject({
      mode: "assist",
      budgetTokens: 1000,
      fusedChunks: 10,
      truncated: true,
    });
    expect(stats.injectedChunks).toBe(injectedCount);
    expect(stats.usedTokens).toBeLessThanOrEqual(1000);
    // 超预算分片：results 只含实际注入的块（分片清单语义不变，被省略的块不进列表）。
    expect(stats.results).toHaveLength(injectedCount);
    expect(stats.results?.map(entry => entry.ordinal)).toEqual(
      Array.from({ length: injectedCount }, (_, index) => index + 1),
    );
  });
  it("超预算 + 提炼模型可用：分段压缩注入，stats 标注 distilled/batches", async () => {
    const candidates = Array.from({ length: 10 }, (_, index) => (
      fakeChunk({ id: `c${index}`, ordinal: index, text: `${"证据".repeat(200)}-${index}` })
    ));
    const distillModel = vi.fn(async ({ batch }: { batch: string }) => `提炼(${batch.length}字)`);
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      budgetTokens: 1000,
      deps: {
        decomposeModel: null,
        distillModel,
        retrieve: async () => fakeRetrieval(candidates),
      },
    });
    expect(distillModel).toHaveBeenCalled();
    expect(block).toContain("distilled from evidence blocks");
    expect(block).not.toContain("Shard manifest");
    expect(stats.distilled).toBe(true);
    expect(typeof stats.distillBatches).toBe("number");
    expect((stats.distillBatches ?? 0)).toBeGreaterThan(0);
    expect(stats.truncated).toBe(false);
    expect(stats.usedTokens).toBeLessThanOrEqual(1000);
  });

  it("超预算 + 未配置提炼模型：退回分片清单并在 stats 留痕", async () => {
    const candidates = Array.from({ length: 10 }, (_, index) => (
      fakeChunk({ id: `c${index}`, ordinal: index, text: `${"证据".repeat(200)}-${index}` })
    ));
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "assist",
      budgetTokens: 1000,
      deps: {
        decomposeModel: null,
        distillModel: null,
        retrieve: async () => fakeRetrieval(candidates),
      },
    });
    expect(block).toContain("Shard manifest");
    expect(stats.distilled).toBeUndefined();
    expect(stats.distillDegradedReason).toBe("distill model not configured");
    expect(stats.truncated).toBe(true);
  });

  it("超预算 + 提炼失败：退回分片清单并携带失败原因", async () => {
    const candidates = Array.from({ length: 10 }, (_, index) => (
      fakeChunk({ id: `c${index}`, ordinal: index, text: `${"证据".repeat(200)}-${index}` })
    ));
    const distillModel = vi.fn(async () => {
      throw new Error("rate limited");
    });
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      budgetTokens: 1000,
      deps: {
        decomposeModel: null,
        distillModel,
        retrieve: async () => fakeRetrieval(candidates),
      },
    });
    expect(block).toContain("Shard manifest");
    expect(stats.distillDegradedReason).toContain("rate limited");
    expect(stats.truncated).toBe(true);
  });


  it("results.firstLine 取块正文首行并截断到 ~120 字符", async () => {
    const longLine = "长".repeat(200);
    const { stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      deps: {
        decomposeModel: null,
        distillModel: null,
        retrieve: async () => fakeRetrieval([
          fakeChunk({ text: `${longLine}\n第二行不进 firstLine` }),
        ]),
      },
    });
    expect(stats.results).toEqual([{
      ordinal: 1,
      sourceName: "源",
      chunkOrdinal: 1,
      firstLine: `${"长".repeat(120)}…`,
    }]);
  });

  it("部分子查询失败：hits 按下标对齐（失败记 0），块内标注 partially unavailable", async () => {
    const callModel: DecomposeModel = async () => validOutput(["成功子查询", "失败子查询"]);
    let call = 0;
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      deps: {
        decomposeModel: callModel,
        distillModel: null,
        retrieve: async ({ query }) => {
          call += 1;
          if (query === "失败子查询") throw new Error("boom");
          return fakeRetrieval([fakeChunk({ id: `c-${call}`, text: "证据" }), fakeChunk({ id: `d-${call}`, text: "证据二" }), fakeChunk({ id: `e-${call}`, text: "证据三" })]);
        },
      },
    });
    expect(block).toContain("[knowledge retrieval partially unavailable: boom]");
    expect(stats.subQueries).toEqual(["成功子查询", "失败子查询"]);
    expect(stats.subQueryHits).toEqual([3, 0]);
    expect(stats.retrievalMode).toBe("fts");
    expect(stats.degraded).toBe(false);
    // 直检通道（原问题，与拆解并行）与成功子查询各贡献 3 条不同 id 候选。
    expect(stats.fusedChunks).toBe(6);
    expect(stats.unavailableReason).toBeUndefined();
  });

  it("子查询检索并行执行且结果保序（慢查询先发起不改变 hits 下标对齐）", async () => {
    const callModel: DecomposeModel = async () => validOutput(["子查询甲", "子查询乙", "子查询丙"]);
    let inFlight = 0;
    let maxInFlight = 0;
    const { stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      deps: {
        decomposeModel: callModel,
        distillModel: null,
        retrieve: async ({ query }) => {
          inFlight += 1;
          maxInFlight = Math.max(maxInFlight, inFlight);
          // 乙最慢、甲次之：完成顺序与发起顺序相反，验证合并只依赖输入顺序。
          await new Promise(resolve => setTimeout(resolve, query === "子查询乙" ? 30 : query === "子查询甲" ? 15 : 5));
          inFlight -= 1;
          return fakeRetrieval([fakeChunk({ id: `c-${query}`, text: query })]);
        },
      },
    });
    // 直检通道 + 三个子查询同时发起（串行实现 maxInFlight 恒为 1）。
    expect(maxInFlight).toBe(4);
    expect(stats.subQueries).toEqual(["子查询甲", "子查询乙", "子查询丙"]);
    expect(stats.subQueryHits).toEqual([1, 1, 1]);
    // 直检（c-问题）与三条子查询候选一并融合。
    expect(stats.fusedChunks).toBe(4);
  });

  it("慢拆解不阻塞直检：原问题检索与拆解 LLM 并行，先于拆解完成发起", async () => {
    const events: string[] = [];
    const callModel: DecomposeModel = async () => {
      events.push("decompose:start");
      await new Promise(resolve => setTimeout(resolve, 20));
      events.push("decompose:end");
      return validOutput(["拆解子查询"]);
    };
    const { block } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      deps: {
        decomposeModel: callModel,
        distillModel: null,
        retrieve: async ({ query }) => {
          events.push(`retrieve:${query}`);
          return fakeRetrieval([fakeChunk({ id: `c-${query}`, text: `证据-${query}` })]);
        },
      },
    });
    // 直检在拆解完成前就已发起（并行结构）；两条通道的候选都进注入块。
    expect(events.indexOf("retrieve:问题")).toBeGreaterThanOrEqual(0);
    expect(events.indexOf("retrieve:问题")).toBeLessThan(events.indexOf("decompose:end"));
    expect(block).toContain("证据-问题");
    expect(block).toContain("证据-拆解子查询");
  });

  it("降级单查询路径只检索一次：等值子查询复用直检结果不重复检索", async () => {
    const queries: string[] = [];
    await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      deps: {
        decomposeModel: null,
        distillModel: null,
        retrieve: async ({ query }) => {
          queries.push(query);
          return fakeRetrieval([fakeChunk({ text: "证据" })]);
        },
      },
    });
    expect(queries).toEqual(["问题"]);
  });

  it("直检失败但子查询成功：失败显式留痕，hits 仍对齐子查询", async () => {
    const callModel: DecomposeModel = async () => validOutput(["子查询甲"]);
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      deps: {
        decomposeModel: callModel,
        distillModel: null,
        retrieve: async ({ query }) => {
          if (query === "问题") throw new Error("direct boom");
          return fakeRetrieval([fakeChunk({ id: "c-1", text: "证据" })]);
        },
      },
    });
    expect(block).toContain("[knowledge retrieval partially unavailable: direct boom]");
    expect(stats.subQueries).toEqual(["子查询甲"]);
    expect(stats.subQueryHits).toEqual([1]);
    expect(stats.fusedChunks).toBe(1);
    expect(stats.unavailableReason).toBeUndefined();
  });

  it("检索全失败：显式标注不可用而不是静默跳过；stats 带 unavailableReason", async () => {
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      deps: {
        decomposeModel: null,
        distillModel: null,
        retrieve: async () => {
          throw new Error("boom");
        },
      },
    });
    expect(block).toContain("[knowledge retrieval unavailable: boom]");
    expect(block).toContain("{{cite:N}}");
    expect(stats).toMatchObject({
      mode: "qa",
      retrievalMode: "none",
      subQueries: ["问题"],
      subQueryHits: [0],
      fusedChunks: 0,
      injectedChunks: 0,
      truncated: false,
      usedTokens: 0,
      unavailableReason: "boom",
    });
    // 零注入：results 为空数组（unavailable 降级路径则整个字段缺席，见 engine 门面）。
    expect(stats.results).toEqual([]);
  });

  it("被引笔记本无 ready 源：显式标注并在 stats 记录原因", async () => {
    const { block, stats } = await buildKnowledgeContextInjection({
      question: "问题",
      mode: "qa",
      deps: {
        decomposeModel: null,
        distillModel: null,
        retrieve: async () => ({ candidates: [], sources: [], retrievalMode: "fts" }),
      },
    });
    expect(block).toContain("[knowledge retrieval unavailable: no ready sources in the referenced notebooks]");
    // 检索本身成功执行（空范围 fts，零候选）："none" 只留给全部子查询失败的路径。
    expect(stats.retrievalMode).toBe("fts");
    expect(stats.fusedChunks).toBe(0);
    expect(stats.unavailableReason).toBe("no ready sources in the referenced notebooks");
  });
});

describe("retrieveForNotebooks 边界与配置（真实 KnowledgeManager）", () => {
  async function setupManager() {
    const manager = new KnowledgeManager({ lingxiHome: tempHome() });
    managers.push(manager);
    return manager;
  }

  async function addReadySource(
    manager: KnowledgeManager,
    studioId: string,
    notebookId: string,
    text: string,
    displayName: string,
  ) {
    const imported = await manager.importPastedText({ studioId, notebookId, text, displayName });
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    manager.queryService.indexArtifactForIngestion(studioId, artifact.id);
    return { imported, artifact };
  }

  it("检索严格限定被引用笔记本：未引用笔记本的源不出现", async () => {
    const manager = await setupManager();
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "甲笔记本" });
    const notebookB = manager.createNotebook({ studioId, name: "乙笔记本" });
    await addReadySource(manager, studioId, notebookA.id, "苹果项目的交付日期是九月十五日。", "苹果.txt");
    await addReadySource(manager, studioId, notebookB.id, "火星项目的预算是八百万元。", "火星.txt");

    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebookA.id],
      question: "项目 交付",
    });
    expect(result.sources.map(source => source.sourceName)).toEqual(["苹果.txt"]);
    expect(result.candidates.length).toBeGreaterThan(0);
    expect(result.candidates.every(chunk => chunk.notebookId === notebookA.id)).toBe(true);
    expect(result.candidates.some(chunk => chunk.text.includes("火星"))).toBe(false);
  });

  it("查询嵌入按笔记本配置的嵌入模型路由（多笔记本不同模型各自分发）", async () => {
    // 记录每次嵌入调用使用的 modelRef：embA/embB 两个模型按笔记本各自分发。
    const embedCalls: string[] = [];
    const embedTextsForModel = async (request: { texts: string[]; modelRef: { id: string } }) => {
      embedCalls.push(request.modelRef.id);
      const vector = new Array(8).fill(0);
      vector[0] = 1;
      return {
        vectors: request.texts.map(() => vector),
        dimensions: 8,
        model: { provider: "fake", id: request.modelRef.id, api: "openai", dimensions: 8 },
      };
    };
    const manager = new KnowledgeManager({ lingxiHome: tempHome(), embedTextsForModel });
    managers.push(manager);
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "甲" });
    const notebookB = manager.createNotebook({ studioId, name: "乙" });
    manager.updateNotebookSettings({ studioId, notebookId: notebookA.id, embeddingModelRef: { id: "embA", provider: "fake" } });
    manager.updateNotebookSettings({ studioId, notebookId: notebookB.id, embeddingModelRef: { id: "embB", provider: "fake" } });
    await addReadySource(manager, studioId, notebookA.id, "苹果项目的交付日期是九月十五日。", "苹果.txt");
    await addReadySource(manager, studioId, notebookB.id, "火星项目的预算是八百万元。", "火星.txt");

    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebookA.id, notebookB.id],
      question: "项目",
    });
    // 两个笔记本的查询向量分别用各自配置的嵌入模型（与索引侧同一模型，
    // 向量命中同一 model_key 分区——修复"查询侧全局模型与索引侧不一致"）。
    // 每模型 ≥1 次（查询嵌入 + 可能的懒构建向量，均路由到该笔记本的模型）；
    // 关键断言：任何调用都不带另一个笔记本的模型（不串模型）。
    expect(embedCalls.filter(id => id === "embA").length).toBeGreaterThanOrEqual(1);
    expect(embedCalls.filter(id => id === "embB").length).toBeGreaterThanOrEqual(1);
    expect(embedCalls.every(id => id === "embA" || id === "embB")).toBe(true);
    expect(result.retrievalMode).toBe("hybrid");
  });

  it("未配置嵌入模型的笔记本走纯 FTS（retrievalMode 如实标注，不静默换模型）", async () => {
    const manager = new KnowledgeManager({
      lingxiHome: tempHome(),
      embedTextsForModel: async () => null,
    });
    managers.push(manager);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    await addReadySource(manager, studioId, notebook.id, "苹果项目的交付日期是九月十五日。", "苹果.txt");

    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "苹果",
    });
    expect(result.retrievalMode).toBe("fts");
    expect(result.candidates.length).toBeGreaterThan(0);
  });

  it("topK 按笔记本配置解析（retrievalTopK=1 时候选不超过 1）", async () => {
    const manager = await setupManager();
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    await addReadySource(manager, studioId, notebook.id,
      "苹果项目的交付日期是九月十五日。火星项目的预算是八百万元。蓝山项目的负责人是李雷。",
      "多主题.txt");
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, retrievalTopK: 1 });

    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "项目",
    });
    expect(result.candidates.length).toBeLessThanOrEqual(1);
  });

  it("rerank 仅在解析链给出引用时执行（hybrid 路径）", async () => {
    const rerank = vi.fn(async () => null);
    // rerank 只作用于 hybrid 候选（融合核心既有语义）：接伪嵌入让检索走 hybrid。
    const fakeEmbedder = async ({ texts }: { texts: string[] }) => ({
      vectors: texts.map((text) => {
        const vector = new Array(8).fill(0);
        vector[text.length % 8] = (text.length % 7) + 1;
        return vector;
      }),
      dimensions: 8,
      model: { provider: "fake", id: "emb-1", api: "openai", dimensions: 8 },
    });
    const manager = new KnowledgeManager({ lingxiHome: tempHome(), embedTextsForModel: fakeEmbedder, rerank });
    managers.push(manager);
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    // v8 起查询嵌入按笔记本嵌入引用路由：未配置嵌入模型 → 纯 FTS → rerank 不触发。
    await addReadySource(manager, studioId, notebook.id, "苹果项目的交付日期是九月十五日。", "苹果.txt");

    await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "苹果",
    });
    expect(rerank).not.toHaveBeenCalled();

    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: { id: "emb-1", provider: "fake" },
      rerankModelRef: { id: "rr-1", provider: "fake" },
    });
    await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "苹果",
    });
    expect(rerank).toHaveBeenCalled();
  });

  it("多笔记本检索并行执行，合并按 notebookIds 顺序轮转交错", async () => {
    const manager = await setupManager();
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "甲笔记本" });
    const notebookB = manager.createNotebook({ studioId, name: "乙笔记本" });
    await addReadySource(manager, studioId, notebookA.id, "苹果项目的交付日期是九月十五日。", "苹果.txt");
    await addReadySource(manager, studioId, notebookB.id, "火星项目的预算是八百万元。", "火星.txt");

    // 观测检索核心的并发度：串行实现恒为 1，并行实现两个笔记本同时发起。
    const service = manager.queryService as any;
    const originalRetrieve = service.retrieve.bind(service);
    let inFlight = 0;
    let maxInFlight = 0;
    service.retrieve = async (input: any) => {
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await new Promise(resolve => setTimeout(resolve, 10));
      try {
        return await originalRetrieve(input);
      } finally {
        inFlight -= 1;
      }
    };
    try {
      const result = await manager.queryService.retrieveForNotebooks({
        studioId,
        notebookIds: [notebookA.id, notebookB.id],
        question: "项目",
      });
      expect(maxInFlight).toBe(2);
      // 确定性合并：rank 0 层按 notebookIds 顺序轮转（两个笔记本各有命中）。
      expect(result.candidates.map(chunk => chunk.notebookId)).toEqual([notebookA.id, notebookB.id]);
      expect(result.sources.map(source => source.sourceName)).toEqual(["苹果.txt", "火星.txt"]);
    } finally {
      service.retrieve = originalRetrieve;
    }
  });

  it("markdown 源的候选带 headingPath 定位头与源清单首章节标题", async () => {
    const root = tempHome();
    const manager = await setupManager();
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "文档" });
    // 外部文件须在 lingxiHome 之外（同级临时目录），lingxiHome 内部路径被导入安全拦截。
    const outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-injector-outside-"));
    tempDirs.push(outsideDir);
    const filePath = path.join(outsideDir, "notes.md");
    fs.writeFileSync(filePath, "# 交付计划\n\n苹果项目的交付日期是九月十五日。\n\n## 风险\n\n火星项目预算八百万。\n");
    const imported = await manager.importFile({ studioId, notebookId: notebook.id, filePath });
    const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
    manager.queryService.indexArtifactForIngestion(studioId, artifact.id);

    const { block } = await buildKnowledgeContextInjection({
      question: "苹果 交付",
      mode: "qa",
      budgetTokens: 5,
      deps: {
        decomposeModel: null,
        distillModel: null,
        retrieve: ({ query }) => manager.queryService.retrieveForNotebooks({
          studioId,
          notebookIds: [notebook.id],
          question: query,
        }),
      },
    });
    expect(block).toContain("heading: 交付计划");
    // 超预算 → 分片清单出现，且清单带源名/chunk 数/ordinal 范围/首章节标题。
    expect(block).toContain("Shard manifest");
    expect(block).toContain("first heading: 交付计划");
    expect(block).toContain(`ordinals 1-`);
  });
});
