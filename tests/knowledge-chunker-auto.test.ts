import { describe, expect, it } from "vitest";
import {
  KNOWLEDGE_CHUNK_FALLBACK_CONTEXT_TOKENS,
  MAX_KNOWLEDGE_CHUNK_TARGET_CHARS,
  MIN_KNOWLEDGE_CHUNK_TARGET_CHARS,
  computeAutoChunkTargetChars,
} from "../lib/knowledge/chunker.ts";

describe("computeAutoChunkTargetChars（嵌入上下文 ×80% 自动分块）", () => {
  it("常规窗口：floor(window × 0.8)", () => {
    expect(computeAutoChunkTargetChars(8192)).toBe(6553);
    expect(computeAutoChunkTargetChars(32768)).toBe(26214);
    // 128000 × 0.8 = 102400 超出 MAX 边界，被夹到 100000
    expect(computeAutoChunkTargetChars(128000)).toBe(100000);
  });

  it("窗口未知/非法回退 8192", () => {
    expect(computeAutoChunkTargetChars(null)).toBe(6553);
    expect(computeAutoChunkTargetChars(undefined)).toBe(6553);
    expect(computeAutoChunkTargetChars(0)).toBe(6553);
    expect(computeAutoChunkTargetChars(-5)).toBe(6553);
    expect(computeAutoChunkTargetChars(Number.NaN)).toBe(6553);
    expect(computeAutoChunkTargetChars(Number.POSITIVE_INFINITY)).toBe(6553);
  });

  it("极小/极大窗口夹在 MIN/MAX 边界", () => {
    expect(computeAutoChunkTargetChars(10)).toBe(MIN_KNOWLEDGE_CHUNK_TARGET_CHARS);
    expect(computeAutoChunkTargetChars(1_000_000)).toBe(MAX_KNOWLEDGE_CHUNK_TARGET_CHARS);
  });

  it("最保守口径下任何窗口的结果都不超嵌入输入上限", () => {
    for (const window of [1000, 8192, 32768, 128000]) {
      const target = computeAutoChunkTargetChars(window);
      // 1 token = 1 字符：字符数 ≤ 窗口数即不超上限；80% 比例再留 20% 余量。
      expect(target).toBeLessThanOrEqual(Math.floor(window * 0.8) + 1);
    }
    expect(KNOWLEDGE_CHUNK_FALLBACK_CONTEXT_TOKENS).toBe(8192);
  });
});
