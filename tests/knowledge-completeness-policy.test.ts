import { describe, expect, it } from "vitest";
import { deriveKnowledgeCompletenessPolicy } from "../lib/knowledge/research/completeness-policy.ts";

const derive = (question: string, mode: "fast" | "detailed" = "detailed", selectedNotebookCount = 1, selectedSourceCount = 1) =>
  deriveKnowledgeCompletenessPolicy({ mode, question, selectedNotebookCount, selectedSourceCount });

describe("用户完整性意图的确定性最低策略", () => {
  it.each([
    "检查全文", "概括全书", "阅读整本资料", "提取全部风险", "核对所有记录", "说明每一个例外",
    "有没有任何未付款项", "是否存在任何冲突", "该事项是否从未发生", "是否没有授权记录", "有没有遗漏",
    "列出所有建议", "统计所有出现的位置", "核对所有提到预算的内容", "从头到尾检查说明书",
  ])("锁定的中文范围要求：%s", question => {
    expect(derive(question)).toBe("scope_complete");
  });

  it.each([
    "List all exceptions.", "Check every occurrence.", "Read the entire report.", "Review the whole book.",
    "Are there any contradictory records?", "Was this never approved?", "Check for an omission.", "Find omissions.",
    "Review the full text.", "Review the full-text contract.", "Read from beginning to end.",
    "Check the report from start to finish.", "Is anything missing?", "Was anything omitted?",
    "Is there no authorization record?", "Are there no recorded exceptions?", "LIST ALL RECORDS", "核对 ＡＬＬ entries",
  ])("英文范围和否定要求：%s", question => {
    expect(derive(question)).toBe("scope_complete");
  });

  it.each([
    "逐章核对结论", "说明每一章的风险", "逐节分析", "核对每个章节", "比较前后章节", "核对所有相关章节",
    "Review chapter-by-chapter.", "Check chapter by chapter.", "Read section-by-section.", "Check section by section.",
    "Review every chapter.", "Check each chapter.", "Review every section.", "Check each section.",
    "Review all relevant chapters.", "Read ALL relevant sections.", "Check previous and next chapters.",
    "Compare preceding and following chapters.", "Check the chapters before and after.",
  ])("章节专属要求：%s", question => {
    expect(derive(question)).toBe("relevant_sections_complete");
  });

  it.each(["each", "every", "all"].flatMap(quantifier =>
    ["chapter", "chapters", "section", "sections"].map(section => `Review ${quantifier} relevant ${section}.`),
  ))("英文相关章节的量词和单复数：%s", question => {
    expect(derive(question)).toBe("relevant_sections_complete");
    expect(derive(`${question} Check all exceptions.`)).toBe("scope_complete");
  });

  it.each([
    "检查所有相关章节，说明所有相关章节的结论",
    "逐章分析所有相关章节，并比较前后章节",
    "Check every chapter and all relevant sections.",
  ])("章节专属短语中的泛词不能误升整范围：%s", question => {
    expect(derive(question)).toBe("relevant_sections_complete");
  });

  it.each([
    "检查所有相关章节，并列出所有例外", "全文检查所有相关章节", "所有相关章节之外有没有遗漏",
    "逐章阅读整本资料", "Review all relevant chapters and all exceptions.",
    "Check every section of the whole report.", "Check chapter-by-chapter for any omission.",
    "全文检查 all relevant sections", "Check 所有相关章节 across the entire book.",
    "Check all relevant sections and whether anything is missing.",
    "Check all relevant sections and whether anything was omitted.",
  ])("章节短语外仍有范围要求时升级：%s", question => {
    expect(derive(question)).toBe("scope_complete");
  });

  it.each(["is", "was", "has been", "have been"])("遗漏从句允许助动词：%s", auxiliary => {
    expect(derive(`Check whether anything ${auxiliary} omitted.`)).toBe("scope_complete");
    expect(derive(`Check all relevant sections and whether anything ${auxiliary} omitted.`)).toBe("scope_complete");
  });

  it("单独询问是否存在遗漏仍要求检查整个范围", () => {
    expect(derive("Check whether anything is missing.")).toBe("scope_complete");
  });

  it.each([
    "预算金额是多少", "比较甲乙方案的价格", "为什么提案没有获得批准", "为什么价格不是十二万元",
    "What is the overall budget?", "Summarize the callback example.", "Who called the supplier?",
    "Which company owns this project?", "Explain the anniversary schedule.", "Nevertheless, what is the price?",
    "Why was the proposal not approved?", "Compare the chapter titles.", "Explain the section heading.",
  ])("普通事实、否定原因或英文子串不误判：%s", question => {
    expect(derive(question)).toBe("source_diverse");
  });

  it.each([[1, 2], [2, 2], [4, 12]])("多来源比较仍为来源多样性最低要求（%i 本/%i 源）", (notebooks, sources) => {
    expect(derive("Compare the cost and support of the two plans.", "detailed", notebooks, sources)).toBe("source_diverse");
  });

  it.each([
    "全文检查所有相关章节有没有遗漏", "逐章和逐节核对", "Check all records for any omission, chapter-by-chapter.",
    "Compare the whole report with every source.", "预算金额是多少",
  ])("快速模式优先于完整性要求：%s", question => {
    expect(derive(question, "fast", 4, 12)).toBe("best_effort");
  });
});
