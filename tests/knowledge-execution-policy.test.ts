import { describe, expect, it } from "vitest";
import { resolveKnowledgeExecutionPolicy } from "../shared/knowledge-execution.ts";
import {
  normalizeKnowledgeRefs,
  normalizeLegacyKnowledgeReferenceMode,
} from "../shared/knowledge-refs.ts";

describe("知识执行策略", () => {
  it("快速模式固定使用本地路径和 1200ms 准入期限", () => {
    expect(resolveKnowledgeExecutionPolicy({
      mode: "fast",
      question: "列出所有章节的变化",
      selectedNotebookCount: 5,
      selectedSourceCount: 30,
    })).toEqual({
      mode: "fast",
      path: "fast_local",
      completenessPolicy: "best_effort",
      responseDetail: "normal",
      retrievalDeadlineMs: 1200,
    });
  });

  it("详细模式使用研究路径且没有快速检索期限", () => {
    expect(resolveKnowledgeExecutionPolicy({
      mode: "detailed",
      question: "比较两份资料",
      selectedNotebookCount: 2,
      selectedSourceCount: 2,
    })).toEqual({
      mode: "detailed",
      path: "detailed_research",
      completenessPolicy: "source_diverse",
      responseDetail: "detailed",
      retrievalDeadlineMs: null,
    });
  });

  it.each([
    ["阅读全文，列出所有例外", "scope_complete"],
    ["逐章核对实施时间", "relevant_sections_complete"],
    ["Review every relevant section", "relevant_sections_complete"],
    ["Check all relevant sections and whether anything is missing.", "scope_complete"],
    ["Is there any exception in this book?", "scope_complete"],
  ])("正式唯一策略入口按用户范围要求设置最低策略：%s", (question, completenessPolicy) => {
    expect(resolveKnowledgeExecutionPolicy({ mode: "detailed", question, selectedNotebookCount: 1, selectedSourceCount: 3 }))
      .toEqual({ mode: "detailed", path: "detailed_research", completenessPolicy,
        responseDetail: "detailed", retrievalDeadlineMs: null });
    expect(resolveKnowledgeExecutionPolicy({ mode: "fast", question, selectedNotebookCount: 1, selectedSourceCount: 3 }))
      .toEqual({ mode: "fast", path: "fast_local", completenessPolicy: "best_effort",
        responseDetail: "normal", retrievalDeadlineMs: 1200 });
  });

  it.each(["qa", "assist"])("历史 %s 读取仍归一为详细，生产输入仍拒绝旧值", (mode) => {
    const normalized = normalizeLegacyKnowledgeReferenceMode(mode);
    expect(normalized).toBe("detailed");
    expect(() => normalizeKnowledgeRefs({ notebookIds: ["notebook-a"], mode }))
      .toThrow('knowledgeRefs.mode must be "fast" or "detailed"');
    expect(resolveKnowledgeExecutionPolicy({
      mode: normalized!,
      question: "历史问题",
      selectedNotebookCount: 1,
      selectedSourceCount: 1,
    }).path).toBe("detailed_research");
  });

  it.each(["fast", "detailed"] as const)("网络协议仍接受 %s", (mode) => {
    expect(normalizeKnowledgeRefs({ notebookIds: ["notebook-a"], mode }))
      .toEqual({ notebookIds: ["notebook-a"], mode });
    expect(normalizeLegacyKnowledgeReferenceMode(mode)).toBe(mode);
  });

  it("历史非法模式仍显式拒绝", () => {
    expect(normalizeLegacyKnowledgeReferenceMode("unknown")).toBeNull();
  });
});
