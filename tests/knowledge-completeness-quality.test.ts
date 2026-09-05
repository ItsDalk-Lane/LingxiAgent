import { describe, expect, it } from "vitest";
import { COMPLETENESS_QUALITY_CASES, runCompletenessQualityCase } from "./helpers/knowledge-completeness-quality-fixture.ts";

describe("P3完整性和详细证据质量门禁", () => {
  it.each(COMPLETENESS_QUALITY_CASES)("%s：真实研究、逐单元检查和最终清单", async name => {
    const result = await runCompletenessQualityCase(name);
    expect(result.metrics.citationValidityRate).toBe(1);
    expect(result.citationCount).toBeGreaterThan(0);
    expect(result.manifestContainsOnlyReadEvidence).toBe(true);
    expect(result.metrics.unavailableUnitCount).toBe(result.corpus.expectedUnavailable);
    if (result.corpus.expectedUnavailable === 0) {
      expect(result.completeness).toMatchObject({ exact: true, coverageRatio: 1 });
      expect(result.metrics.counterEvidenceCheckRate).toBe(1);
    } else {
      expect(result.completeness.exact).toBe(false);
      expect(result.metrics.coverageRatio).toBeLessThan(1);
      expect(result.block).toContain("无法证明完整不存在");
      expect(result.block).not.toContain("才允许说“在所选完整范围中不存在");
    }
    if (result.corpus.expectedConflict) {
      expect(result.metrics.conflictDetectionRate).toBe(1);
      expect(result.needStates).toContain("conflicted");
      expect(result.run.status).toBe("partial");
    } else if (!result.corpus.expectedUnavailable) {
      expect(result.metrics.requiredNeedCompletionRate).toBe(1);
      expect(result.run.status).toBe("completed");
    }
    for (const text of result.corpus.expectedText) expect(result.canonicalTexts.some(quote => quote.includes(text))).toBe(true);
    if (name === "repeated-fact") expect(result.canonicalTexts.filter(text => text.includes("赵六"))).toHaveLength(2);
    expect(result.telemetry.timeToFirstResearchAction).not.toBeNull();
    expect(result.telemetry.timeToFirstResearchAction!).toBeLessThanOrEqual(500);
    expect(result.metrics.researchRounds).toBeLessThanOrEqual(4);
    expect(result.telemetry.toolCalls).toBeLessThanOrEqual(32);
    expect(result.telemetry.modelCalls).toBe(result.telemetry.modelTurns.length + 1);
    expect(result.telemetry.finalSynthesisMs).toBeGreaterThanOrEqual(0);
    expect(result.telemetry.finalSynthesis.observedCalls).toBe(1);
    expect(result.telemetry.externalModelCalls).toBe(0);
  });

  it("即使全部资料可读，漏掉最后单元也必须限制否定措辞", async () => {
    const result = await runCompletenessQualityCase("absolute-absence", { omitLast: true });
    expect(result.metrics.coverageRatio).toBeLessThan(1);
    expect(result.completeness.exact).toBe(false);
    expect(result.block).toContain("在已检查的范围内未发现");
    expect(result.block).not.toContain("才允许说“在所选完整范围中不存在");
    expect(result.metrics.citationValidityRate).toBe(1);
    expect(result.run.status).toBe("partial");
  });
});
