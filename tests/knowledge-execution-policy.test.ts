import { describe, expect, it } from "vitest";
import { resolveKnowledgeExecutionPolicy } from "../shared/knowledge-execution.ts";
import {
  normalizeKnowledgeRefs,
  normalizeLegacyKnowledgeReferenceMode,
} from "../shared/knowledge-refs.ts";

describe("知识执行策略", () => {
  it("旧快速设置使用连续查阅，不再预先检索", () => {
    expect(resolveKnowledgeExecutionPolicy({
      mode: "fast",
      question: "列出所有章节的变化",
      selectedNotebookCount: 5,
      selectedSourceCount: 30,
    })).toEqual({
      mode: "auto",
      path: "conversation",
      completenessPolicy: "best_effort",
      responseDetail: "normal",
      retrievalDeadlineMs: null,
    });
  });

  it("旧详细设置使用连续查阅，保留回答详细程度", () => {
    expect(resolveKnowledgeExecutionPolicy({
      mode: "detailed",
      question: "比较两份资料",
      selectedNotebookCount: 2,
      selectedSourceCount: 2,
    })).toEqual({
      mode: "auto",
      path: "conversation",
      completenessPolicy: "best_effort",
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
  ])("范围用语不会强制启动独立研究：%s", (question, _legacyPolicy) => {
    expect(resolveKnowledgeExecutionPolicy({ mode: "detailed", question, selectedNotebookCount: 1, selectedSourceCount: 3 }))
      .toEqual({ mode: "auto", path: "conversation", completenessPolicy: "best_effort",
        responseDetail: "detailed", retrievalDeadlineMs: null });
    expect(resolveKnowledgeExecutionPolicy({ mode: "fast", question, selectedNotebookCount: 1, selectedSourceCount: 3 }))
      .toEqual({ mode: "auto", path: "conversation", completenessPolicy: "best_effort",
        responseDetail: "normal", retrievalDeadlineMs: null });
  });

  it.each(["qa", "assist"])("历史 %s 读取仍归一为详细，生产输入仍拒绝旧值", (mode) => {
    const normalized = normalizeLegacyKnowledgeReferenceMode(mode);
    expect(normalized).toBe("detailed");
    expect(() => normalizeKnowledgeRefs({ notebookIds: ["notebook-a"], mode }))
      .toThrow('knowledgeRefs.mode must be "auto", "fast" or "detailed"');
    expect(resolveKnowledgeExecutionPolicy({
      mode: normalized!,
      question: "历史问题",
      selectedNotebookCount: 1,
      selectedSourceCount: 1,
    }).path).toBe("conversation");
  });

  it.each(["auto", "fast", "detailed"] as const)("网络协议仍接受 %s", (mode) => {
    expect(normalizeKnowledgeRefs({ notebookIds: ["notebook-a"], mode }))
      .toEqual({ notebookIds: ["notebook-a"], mode: "auto" });
    expect(normalizeLegacyKnowledgeReferenceMode(mode)).toBe(mode);
  });

  it("历史非法模式仍显式拒绝", () => {
    expect(normalizeLegacyKnowledgeReferenceMode("unknown")).toBeNull();
  });
});
