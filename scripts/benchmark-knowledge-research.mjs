import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { COMPLETENESS_QUALITY_CASES, runCompletenessQualityCase } from "../tests/helpers/knowledge-completeness-quality-fixture.ts";

/** 固定模型动作只衡量宿主链路；在线模型耗时、真实用量和最终回答不得用估算伪装。 */
export async function runKnowledgeResearchBenchmark({ outputPath = null, cases = COMPLETENESS_QUALITY_CASES } = {}) {
  const results = [];
  for (const name of cases) {
    const result = await runCompletenessQualityCase(name);
    results.push({ name, status: result.run.status, metrics: result.metrics, telemetry: result.telemetry,
      exact: result.completeness.exact, citationCount: result.citationCount, validCitations: result.validCitations });
  }
  const report = { schemaVersion: 1, platform: process.platform, arch: process.arch, node: process.version,
    cpu: os.cpus()[0]?.model ?? null, memoryBytes: os.totalmem(), osRelease: os.release(), generatedAt: new Date().toISOString(),
    boundary: "deterministic model actions; real local research runtime; final synthesis uses real AgentSession/HTTP against a local scripted provider; timing and declared usage do not represent paid provider performance", results };
  if (outputPath) { fs.mkdirSync(path.dirname(outputPath), { recursive: true }); fs.writeFileSync(outputPath, JSON.stringify(report, null, 2) + "\n"); }
  for (const result of results) {
    assert.equal(result.metrics.citationValidityRate, 1);
    assert.ok(result.telemetry.timeToFirstResearchAction !== null && result.telemetry.timeToFirstResearchAction <= 500);
    assert.ok(result.metrics.researchRounds <= 4 && result.telemetry.toolCalls <= 32);
    if (result.metrics.unavailableUnitCount === 0) assert.equal(result.metrics.coverageRatio, 1);
  }
  return report;
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const outputArg = process.argv.find(arg => arg.startsWith("--output="));
  console.log(JSON.stringify(await runKnowledgeResearchBenchmark({ outputPath: outputArg?.slice("--output=".length) || null }), null, 2));
}
