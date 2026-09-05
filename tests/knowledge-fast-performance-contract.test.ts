import { describe, expect, it } from "vitest";
import { runKnowledgeFastBenchmark } from "../scripts/benchmark-knowledge-fast.mjs";

describe("快速知识检索性能契约", () => {
  it("固定种子样本覆盖完整生产阶段，普通测试只守确定性上限", async () => {
    const report = await runKnowledgeFastBenchmark({ sizes: [1_000], hotRuns: 3, coldRuns: 2, enforce: false });
    expect(report).toMatchObject({ schemaVersion: 2, seed: "lingxi-knowledge-fast-v2-real-three-grain-index" });
    const result = report.results[0];
    expect(result.size).toBe(1_000);
    expect(result.hot.samples).toHaveLength(3);
    expect(result.cold.samples).toHaveLength(2);
    for (const sample of [...result.hot.samples, ...result.cold.samples]) {
      expect(sample.remoteModelCalls).toBe(0);
      expect(sample.returnedSpans).toBeGreaterThan(0);
      expect(sample.returnedSpans).toBeLessThanOrEqual(8);
      expect(sample.usedTokens).toBeLessThanOrEqual(2_400);
      for (const key of ["scopeCompileMs", "ftsMs", "spanExtractMs", "packMs", "totalMs"] as const) {
        expect(sample[key]).toBeGreaterThanOrEqual(0);
      }
    }
    expect(result.hot.percentiles.totalMs).toEqual(expect.objectContaining({ P50: expect.any(Number),
      P95: expect.any(Number), P99: expect.any(Number) }));
  });
});
