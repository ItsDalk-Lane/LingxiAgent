import { describe, expect, it } from "vitest";
import {
  CJK_TOKENS_PER_CHAR,
  estimateTextTokens,
  trimTextToTokenBudget,
} from "../lib/llm/estimate-text-tokens.ts";

describe("estimateTextTokens（语言感知估算）", () => {
  it("空串与非字符串返回 0", () => {
    expect(estimateTextTokens("")).toBe(0);
    expect(estimateTextTokens(null)).toBe(0);
    expect(estimateTextTokens(undefined)).toBe(0);
    expect(estimateTextTokens(123)).toBe(0);
  });

  it("纯英文按 chars/4 估算", () => {
    const text = "abcdefghijklmnop"; // 16 chars
    expect(estimateTextTokens(text)).toBe(4);
  });

  it("纯中文按 CJK 系数估算（约 1.1 token/字）", () => {
    const text = "知识库检索"; // 5 个 CJK 字符
    expect(estimateTextTokens(text)).toBe(Math.ceil(5 * CJK_TOKENS_PER_CHAR));
  });

  it("中英混合分别累计", () => {
    const cjk = "知识库";
    const other = "abcdefgh"; // 8 chars
    const expected = Math.ceil(3 * CJK_TOKENS_PER_CHAR + 8 / 4);
    expect(estimateTextTokens(cjk + other)).toBe(expected);
  });

  it("日文假名与韩文谚文按 CJK 口径估算", () => {
    const jp = "あいう"; // 平假名
    const kr = "한국어"; // 谚文
    const expected = Math.ceil(6 * CJK_TOKENS_PER_CHAR);
    expect(estimateTextTokens(jp + kr)).toBe(expected);
  });

  it("全角标点按 CJK 口径估算", () => {
    const text = "。，！？"; // 4 个全角标点
    expect(estimateTextTokens(text)).toBe(Math.ceil(4 * CJK_TOKENS_PER_CHAR));
  });

  it("中文估算明显高于旧 chars/4 口径（回归锚点）", () => {
    const text = "这是一个用于验证中文 token 估算不再按英文口径低估的句子。";
    expect(estimateTextTokens(text)).toBeGreaterThan(text.length / 4);
  });
});

describe("trimTextToTokenBudget（按预算截断）", () => {
  it("预算内返回原文", () => {
    expect(trimTextToTokenBudget("abcd", 10)).toBe("abcd");
    expect(trimTextToTokenBudget("知识库", 100)).toBe("知识库");
  });

  it("截断后估算不超过预算", () => {
    const text = "a".repeat(1000);
    const trimmed = trimTextToTokenBudget(text, 10);
    expect(trimmed.length).toBeLessThanOrEqual(40);
    expect(estimateTextTokens(trimmed)).toBeLessThanOrEqual(10);
  });

  it("中文按 CJK 口径截断（远早于 chars/4 位置）", () => {
    const text = "知".repeat(100);
    const trimmed = trimTextToTokenBudget(text, 5);
    // 每字 1.1 token，预算 5 → 最多 4 字
    expect(trimmed.length).toBeLessThanOrEqual(4);
    expect(estimateTextTokens(trimmed)).toBeLessThanOrEqual(5);
  });

  it("零/负预算返回空串", () => {
    expect(trimTextToTokenBudget("abc", 0)).toBe("");
    expect(trimTextToTokenBudget("abc", -1)).toBe("");
  });

  it("截断点单调：预算越大结果越长", () => {
    const text = "mixed 中英 mixed 中英 mixed 中英";
    const t1 = trimTextToTokenBudget(text, 3);
    const t2 = trimTextToTokenBudget(text, 6);
    const t3 = trimTextToTokenBudget(text, 12);
    expect(t1.length).toBeLessThanOrEqual(t2.length);
    expect(t2.length).toBeLessThanOrEqual(t3.length);
  });
});
