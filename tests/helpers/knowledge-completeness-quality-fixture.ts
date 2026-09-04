import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import { performance } from "node:perf_hooks";
import { createResearchQualityFixture, type QualityCorpus } from "./knowledge-research-quality-fixture.ts";
import { researchNeed, requestFinish } from "./knowledge-research-agent-fixture.ts";
import { readKnowledgeCompletenessSummary } from "../../lib/knowledge/research/knowledge-completeness-executor.ts";
import { EvidenceLedger } from "../../lib/knowledge/research/evidence-ledger.ts";
import { measureKnowledgeFinalSynthesis } from "./knowledge-final-synthesis-fixture.ts";
import { UNTRUSTED_EXTERNAL_CONTENT_MARKER } from "../../lib/security/injection-scan.ts";

export const COMPLETENESS_QUALITY_CASES = ["all-person-names", "absolute-absence", "chapter-changes", "all-counterexamples",
  "needs-ocr", "parse-failed", "repeated-fact", "within-source-conflict"] as const;
interface Corpus extends QualityCorpus { query: string; expectedText: string[]; expectedConflict: boolean; expectedUnavailable: number }

/** 模型边界为固定脚本；脚本只消费工具实际给出的正文，不从预期答案向台账写证据。 */
export async function runCompletenessQualityCase(name: typeof COMPLETENESS_QUALITY_CASES[number], options: { omitLast?: boolean } = {}) {
  const corpus = JSON.parse(fs.readFileSync(new URL(`../fixtures/knowledge-completeness/${name}.json`, import.meta.url), "utf8")) as Corpus;
  let needId = "";
  const f = await createResearchQualityFixture(name, async turn => {
    if (turn.options.surface === "knowledge_completeness_worker") {
      const { completenessCheckId: checkId, completenessShardId: shardId } = turn.options.research;
      const read = await turn.call("knowledge_coverage_read", { runId: turn.runId, checkId, shardId });
      assert.equal(read.isError, undefined);
      const units = options.omitLast ? read.units.slice(0, -1) : read.units;
      const results = units.map((unit: { unitId: string; receiptId?: string; status: string }) => {
        if (unit.status !== "available") return { unitId: unit.unitId, status: "unavailable" };
        const start = read.text.indexOf(`unitId: ${unit.unitId};`);
        assert.ok(start >= 0);
        const section = read.text.slice(start).split(UNTRUSTED_EXTERNAL_CONTENT_MARKER)[1];
        assert.equal(typeof section, "string");
        const text = section.replace(/^\n/u, "").replace(/\n$/u, "");
        // 夹具原句均短于引文上限；标题也保留定位，但不把标题误算成人名或结论。
        assert.ok(text.length > 0 && text.length <= 2000);
        const relation = /反例：|冲突记录：/u.test(text) ? "contradicts" : "supports";
        return { unitId: unit.unitId, receiptId: unit.receiptId, status: "relevant",
          evidence: [{ needId, receiptId: unit.receiptId, quote: text, occurrenceIndex: 0, relation, rationale: "逐单元阅读得到的原句" }] };
      });
      if (results.length) assert.equal((await turn.call("knowledge_completeness_mark", { checkId, results })).isError, undefined);
      return;
    }
    if (!needId) {
      assert.equal((await turn.call("knowledge_outline", { scopeId: turn.scopeId })).isError, undefined);
      const created = await turn.call("knowledge_research_update", { runId: turn.runId,
        createNeeds: [researchNeed(corpus.question, { kind: "completeness", requireAllRelevantUnits: true, requireCounterEvidence: true })] });
      assert.equal(created.isError, undefined); needId = created.needs[0].id;
      // 搜索仅用来找线索，完整性来自逐单元读取，绝不将候选ID当作证据凭据。
      assert.equal((await turn.call("knowledge_search", { scopeId: turn.scopeId, query: corpus.query, channel: "hybrid" })).isError, undefined);
    }
    await requestFinish(turn);
  }, corpus);
  try {
    const started = performance.now(), output = await f.run(), totalMs = performance.now() - started;
    const runId = output.stats.research!.runId, run = f.research.requireRun(runId);
    const completeness = readKnowledgeCompletenessSummary(f.research, runId)!;
    assert.ok(completeness);
    const ledger = new EvidenceLedger(f.research, { isCompletenessSatisfied: () => completeness.exact });
    const needs = f.research.listNeeds(runId).map(need => ledger.evaluateNeed(runId, need.id)), evidence = f.research.listEvidence(runId);
    const receipts = (f.manager.store.db.prepare("SELECT id FROM knowledge_research_read_receipts WHERE run_id=? AND consumed_at IS NOT NULL").all(runId) as Array<{ id: string }>)
      .map(row => f.research.getReceipt(runId, row.id));
    let citationCount = 0, validCitations = 0;
    const canonicalTexts: string[] = [];
    const hash = (text: string) => crypto.createHash("sha256").update(text).digest("hex");
    for (const entry of output.evidence.entries) for (const span of entry.blockSpans) {
      citationCount++;
      const block = f.manager.store.getArtifactBlocksByIds({ studioId: f.request.compiledScope.studioId,
        parseArtifactId: entry.parseArtifactId, blockIds: [span.blockId] })[0];
      const text = block.text.slice(span.blockStartOffset, span.blockEndOffset);
      const receipt = receipts.find(item => item.sourceId === entry.sourceId && item.parseArtifactId === entry.parseArtifactId
        && item.blockId === span.blockId && item.startOffset <= span.blockStartOffset && item.endOffset >= span.blockEndOffset);
      if (receipt && block.textSha256 === hash(block.text)
        && receipt.canonicalTextSha256 === hash(block.text.slice(receipt.startOffset, receipt.endOffset))
        && evidence.some(item => item.blockId === span.blockId && item.startOffset === span.blockStartOffset
          && item.endOffset === span.blockEndOffset && item.canonicalText === text)) validCitations++;
      canonicalTexts.push(text);
    }
    f.engine.recordKnowledgeEvidenceManifest({ sessionPath: f.request.parentSessionPath, stats: output.stats, evidence: output.evidence });
    const manifest = f.manager.store.getEvidenceManifestByScope({ scopeId: output.stats.scopeId })!;
    assert.equal(manifest.entries.flatMap(entry => entry.blockSpans.flatMap(block => block.spans)).length, citationCount);
    const required = needs.filter(need => need.required), counter = needs.filter(need => need.requireCounterEvidence);
    const metrics = {
      requiredNeedCompletionRate: required.filter(need => ["supported", "not_applicable"].includes(need.status)).length / required.length,
      citationValidityRate: citationCount ? validCitations / citationCount : null,
      conflictDetectionRate: corpus.expectedConflict ? Number(needs.some(need => need.status === "conflicted")) : null,
      counterEvidenceCheckRate: counter.length ? counter.filter(need => need.counterEvidenceChecked).length / counter.length : null,
      coverageRatio: completeness.coverageRatio, unavailableUnitCount: completeness.unavailableUnits,
      researchRounds: run.roundsCompleted, searchCalls: run.searchCalls, readCalls: run.readCalls, delegatedAgents: run.delegatedAgents,
    };
    const actions = f.research.listActions(runId), rounds = f.research.listRounds(runId);
    assert.equal(actions.length, run.toolCallsUsed);
    assert.equal(f.invocations.length, actions.length);
    assert.equal(f.modelTurns.length, f.calls.length);
    assert.ok(actions.every(action => action.status !== "running" && action.completedAt));
    assert.ok(f.sessionPaths.every(file => !fs.existsSync(file)));
    const finalSynthesis = await measureKnowledgeFinalSynthesis(output.block, run.status !== "completed");
    const completed = f.progress.find(item => item.event.type === "knowledge_research_completed");
    return { name, corpus, metrics, completeness, run, canonicalTexts, citationCount, validCitations, block: output.block,
      manifestContainsOnlyReadEvidence: validCitations === citationCount, needStates: needs.map(need => need.status),
      telemetry: { timeToFirstResearchAction: f.progress[0]?.elapsedMs ?? null,
        totalMs, roundDurationsMs: rounds.map(round => ({ round: round.ordinal, durationMs: round.completedAt ? Date.parse(round.completedAt) - Date.parse(round.startedAt) : null })),
        modelTurns: f.modelTurns, workerDurationsMs: f.modelTurns.filter(turn => turn.role === "worker").map(turn => turn.durationMs),
        searches: f.invocations.filter(call => call.name === "knowledge_search").map(call => ({ durationMs: call.durationMs, query: call.params.query })),
        reads: f.invocations.filter(call => ["knowledge_read", "knowledge_grep", "knowledge_coverage_read"].includes(call.name)).map(call => ({ name: call.name, durationMs: call.durationMs })),
        finalContextAssemblyMs: completed ? totalMs - completed.elapsedMs : null,
        finalSynthesisMs: finalSynthesis.durationMs, finalSynthesisStatus: "measured_loopback_provider", finalSynthesis,
        modelCalls: f.modelTurns.length + finalSynthesis.modelCalls, researchModelCalls: f.modelTurns.length, externalModelCalls: 0, toolCalls: actions.length,
        tokens: { measuredProviderUsage: finalSynthesis.usage, usageBoundary: finalSynthesis.boundary, estimatedPromptTokens: f.modelTurns.reduce((sum, turn) => sum + turn.estimatedInputTokens, 0),
          finalContextTokens: output.stats.usedTokens },
        modelBoundary: "deterministic scripted model; real Engine, Agent tools, SQLite, receipts, manifests and cleanup" } };
  } finally { await f.close(); }
}
