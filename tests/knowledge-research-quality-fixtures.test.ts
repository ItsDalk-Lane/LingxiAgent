import { afterEach, describe, expect, it, vi } from "vitest";
import crypto from "node:crypto";
import { DEFAULT_KNOWLEDGE_RESEARCH_BUDGET } from "../shared/knowledge-research.ts";
import { researchNeed, requestFinish, type ResearchModelTurn } from "./helpers/knowledge-research-agent-fixture.ts";
import { createResearchQualityFixture, readQualityQuote } from "./helpers/knowledge-research-quality-fixture.ts";

type Fixture = Awaited<ReturnType<typeof createResearchQualityFixture>>;
type Result = Awaited<ReturnType<Fixture["run"]>>;
const fixtures: Fixture[] = [];
afterEach(async () => { vi.restoreAllMocks(); for (const fixture of fixtures.splice(0).reverse()) await fixture.close(); });
async function fixture(name: string, driver: (turn: ResearchModelTurn) => Promise<unknown>) {
  const f = await createResearchQualityFixture(name, driver); fixtures.push(f); return f;
}
async function createNeeds(turn: ResearchModelTurn, needs: ReturnType<typeof researchNeed>[]) {
  expect((await turn.call("knowledge_outline", { scopeId: turn.scopeId })).isError).toBeUndefined();
  const result = await turn.call("knowledge_research_update", { runId: turn.runId, createNeeds: needs });
  expect(result.isError).toBeUndefined();
  return result.needs as Array<{ id: string; ordinal: number }>;
}
const search = (turn: ResearchModelTurn, query: string, filters: Record<string, unknown> = {}) =>
  turn.call("knowledge_search", { scopeId: turn.scopeId, query, channel: "fts", ...filters });

/** 每个最终定位均回查消费凭据和冻结原文，并真正持久化后再次核对清单。 */
function verifyFinalEvidence(f: Fixture, result: Result) {
  const runId = result.stats.research!.runId;
  const receiptIds = f.manager.store.db.prepare("SELECT id FROM knowledge_research_read_receipts WHERE run_id = ? AND consumed_at IS NOT NULL")
    .all(runId) as Array<{ id: string }>;
  const scope = f.manager.getTurnScope({ scopeId: result.stats.scopeId })!;
  const digest = (text: string) => crypto.createHash("sha256").update(text, "utf8").digest("hex");
  const receipts = receiptIds.map(({ id }) => {
    const receipt = f.research.getReceipt(runId, id);
    expect(receipt.consumedAt).not.toBeNull();
    expect(scope.sources).toContainEqual(expect.objectContaining({ sourceId: receipt.sourceId,
      contentSnapshotId: receipt.contentSnapshotId, parseArtifactId: receipt.parseArtifactId }));
    expect(f.manager.store.getContentSnapshot({ studioId: scope.studioId, snapshotId: receipt.contentSnapshotId }).sourceId).toBe(receipt.sourceId);
    expect(f.manager.store.getParseArtifact({ studioId: scope.studioId, parseArtifactId: receipt.parseArtifactId }).contentSnapshotId).toBe(receipt.contentSnapshotId);
    const block = f.manager.store.getArtifactBlocksByIds({ studioId: scope.studioId,
      parseArtifactId: receipt.parseArtifactId, blockIds: [receipt.blockId] })[0];
    expect(block.textSha256).toBe(digest(block.text));
    const text = block.text.slice(receipt.startOffset, receipt.endOffset);
    expect(receipt.canonicalTextSha256).toBe(digest(text));
    return { receipt, text };
  });
  const ledger = f.research.listEvidence(runId);
  const texts: string[] = [];
  for (const entry of result.evidence.entries) for (const span of entry.blockSpans) {
    const frozen = scope.sources.find(source => source.sourceId === entry.sourceId)!;
    const matching = receipts.find(({ receipt }) => receipt.sourceId === entry.sourceId
      && receipt.contentSnapshotId === frozen.contentSnapshotId && receipt.parseArtifactId === entry.parseArtifactId
      && receipt.blockId === span.blockId && receipt.startOffset <= span.blockStartOffset && receipt.endOffset >= span.blockEndOffset);
    expect(matching, "最终证据必须有已消费的真实原文凭据").toBeDefined();
    const original = matching!.text.slice(span.blockStartOffset - matching!.receipt.startOffset, span.blockEndOffset - matching!.receipt.startOffset);
    expect(ledger.some(item => item.blockId === span.blockId && item.startOffset === span.blockStartOffset
      && item.endOffset === span.blockEndOffset && item.canonicalText === original)).toBe(true);
    texts.push(original);
  }
  f.engine.recordKnowledgeEvidenceManifest({ sessionPath: f.request.parentSessionPath, stats: result.stats, evidence: result.evidence });
  const manifest = f.manager.store.getEvidenceManifestByScope({ scopeId: result.stats.scopeId })!;
  expect(manifest.entries.flatMap(entry => entry.blockSpans.flatMap(item => item.spans))).toHaveLength(texts.length);
  expect(manifest.entries.every(entry => entry.vectorIndexVariantIds.length === 0)).toBe(true);
  expect(JSON.stringify(manifest)).not.toContain("snippet");
  expect(result.block).not.toContain("私有模型推理不得传给下轮或最终回答");
  return { run: f.research.requireRun(runId), needs: f.research.listNeeds(runId), texts, manifest };
}

describe("P2 七组真实研究质量资料", () => {
  it("second-round-clue：首轮只得到中间词，第二轮从账本提取该词重新搜索后才能回答", async () => {
    let round = 0;
    const f = await fixture("second-round-clue", async turn => {
      if (++round === 1) {
        const [need] = await createNeeds(turn, [researchNeed("红桥迁移的交付日期")]);
        const found = await search(turn, "红桥迁移");
        expect(found.hits.map((hit: { sourceId: string }) => hit.sourceId)).toEqual([f.sources.clue.sourceId]);
        expect(JSON.stringify(found)).not.toContain("2026年10月18日");
        const read = await turn.call("knowledge_read", { scopeId: turn.scopeId, sourceId: found.hits[0].sourceId });
        const spans = read.chunks.flatMap((chunk: { spans: Array<{ text: string; receiptId: string }> }) => chunk.spans);
        const span = spans.find((item: { text: string }) => item.text.includes("唯一结算代号"));
        const clue = /「([^」]+)」/.exec(span.text)![1];
        expect((await turn.call("knowledge_research_update", { runId: turn.runId,
          linkEvidence: [{ needId: need.id, receiptId: span.receiptId, quote: clue, relation: "context", rationale: "只能作为下轮定位线索" }] })).isError).toBeUndefined();
        expect((await requestFinish(turn)).accepted).toBe(false);
      } else {
        const state = JSON.parse(turn.prompt.slice(turn.prompt.indexOf("{"), turn.prompt.lastIndexOf("}") + 1));
        const clue = state.ledger.evidence[0].canonicalText;
        const found = await search(turn, clue);
        expect(found.hits.some((hit: { sourceId: string }) => hit.sourceId === f.sources.answer.sourceId)).toBe(true);
        await readQualityQuote(turn, state.ledger.needs[0].id, f.sources.answer.sourceId, "2026年10月18日");
        expect((await requestFinish(turn)).accepted).toBe(true);
      }
    });
    const output = await f.run();
    const checked = verifyFinalEvidence(f, output);
    expect(checked.run).toMatchObject({ status: "completed", roundsCompleted: 2 });
    expect(f.invocations.filter(call => call.name === "knowledge_search").map(call => call.params.query)).toEqual(["红桥迁移", "云杉钥匙"]);
    expect(checked.texts).toContain("2026年10月18日");
    expect(f.calls[1].prompt).toContain("云杉钥匙");
  });

  it("cross-source-comparison：多维比较实际启动两个 Worker，摘要和伪造引文不能冒充已读证据", async () => {
    const quotes = ["松舟年费为十二万元，提供工作日邮件支持。", "竹舟年费为十八万元，提供全天电话支持。"];
    const f = await fixture("cross-source-comparison", async turn => {
      if (turn.role === "worker") {
        const need = f.research.getNeed(turn.runId, turn.options.research.allowedNeedIds[0]);
        const source = need.ordinal === 0 ? f.sources.left : f.sources.right;
        const found = await search(turn, need.ordinal === 0 ? "松舟" : "竹舟", { sourceIds: [source.sourceId] });
        if (need.ordinal === 0) {
          const attempted = await turn.call("knowledge_research_update", { runId: turn.runId,
            linkEvidence: [{ needId: need.id, receiptId: found.hits[0].candidateId, quote: found.hits[0].snippet,
              relation: "supports", rationale: "试图直接引用搜索摘要" }] });
          expect(attempted).toMatchObject({ isError: true, errorCode: "KNOWLEDGE_NOT_FOUND" });
          const read = await turn.call("knowledge_read", { scopeId: turn.scopeId, sourceId: source.sourceId });
          const attemptedQuote = await turn.call("knowledge_research_update", { runId: turn.runId,
            linkEvidence: [{ needId: need.id, receiptId: read.chunks[0].spans[0].receiptId,
              quote: "松舟完全免费并且无条件退款", relation: "supports", rationale: "试图伪造原句" }] });
          expect(attemptedQuote).toMatchObject({ isError: true, errorCode: "KNOWLEDGE_MODEL_OUTPUT_INVALID" });
          const corrected = await turn.call("knowledge_research_update", { runId: turn.runId,
            linkEvidence: [{ needId: need.id, receiptId: read.chunks.flatMap(chunk => chunk.spans).find(span => span.text.includes(quotes[0])).receiptId,
              quote: quotes[0], relation: "supports", rationale: "用同一凭据纠正伪造引文" }] });
          expect(corrected.isError).toBeUndefined();
          return;
        }
        expect((await readQualityQuote(turn, need.id, source.sourceId, quotes[need.ordinal])).isError).toBeUndefined();
        return;
      }
      const needs = await createNeeds(turn, [researchNeed("松舟年费和支持", { kind: "comparison" }), researchNeed("竹舟年费和支持", { kind: "comparison" })]);
      expect((await requestFinish(turn)).accepted).toBe(false);
      expect((await turn.call("knowledge_delegate", { runId: turn.runId,
        tasks: needs.map(need => ({ label: `比较侧${need.ordinal + 1}`, task: "独立检索并读取本侧资料，精确引文入账", needIds: [need.id] })) })).isError).toBeUndefined();
      expect((await requestFinish(turn)).accepted).toBe(true);
    });
    const output = await f.run(), checked = verifyFinalEvidence(f, output);
    expect(checked.run).toMatchObject({ status: "completed", delegatedAgents: 2 });
    expect(f.calls.filter(turn => turn.role === "worker")).toHaveLength(2);
    expect(checked.texts.sort()).toEqual(quotes.sort());
    expect(checked.manifest.entries).toHaveLength(2);
    expect(f.invocations.filter(call => call.result.errorCode === "KNOWLEDGE_NOT_FOUND")).toHaveLength(1);
    expect(f.invocations.filter(call => call.result.errorCode === "KNOWLEDGE_MODEL_OUTPUT_INVALID")).toHaveLength(1);
    expect(output.block).not.toContain("完全免费"); expect(output.block).not.toContain("本页附注标记");
  });

  it("conflicting-sources：冲突数字形成 conflicted 需求，不能靠结束声明抹掉另一侧", async () => {
    let round = 0;
    const f = await fixture("conflicting-sources", async turn => {
      if (++round === 1) {
        const [need] = await createNeeds(turn, [researchNeed("北港工程预算金额")]);
        for (const [key, quote, relation] of [["first", "一百二十万元", "supports"], ["second", "一百八十万元", "contradicts"]] as const) {
          expect((await search(turn, "北港工程", { sourceIds: [f.sources[key].sourceId] })).hits).toHaveLength(1);
          expect((await readQualityQuote(turn, need.id, f.sources[key].sourceId, quote, relation)).isError).toBeUndefined();
        }
      }
      expect((await requestFinish(turn)).accepted).toBe(false);
    });
    const output = await f.run(), checked = verifyFinalEvidence(f, output);
    expect(checked.run).toMatchObject({ status: "partial", stopReason: "no_progress", roundsCompleted: 3 });
    expect(checked.needs[0].status).toBe("conflicted");
    expect(checked.texts.sort()).toEqual(["一百二十万元", "一百八十万元"].sort());
    expect(output.block).toContain("支持与矛盾证据尚未得到一致解释");
  });

  it("counterexample：第一轮通用规则不足以结束，另一章节的真实反例必须保留", async () => {
    let round = 0, needId = "";
    const f = await fixture("counterexample", async turn => {
      round++;
      if (round === 1) needId = (await createNeeds(turn, [researchNeed("所有采购必须提前审批", { kind: "counterexample", requireCounterEvidence: true })]))[0].id;
      if (round <= 2) {
        const heading = round === 1 ? "通用规则" : "紧急例外";
        const pattern = round === 1 ? "所有采购" : "紧急采购";
        const found = await search(turn, pattern, { sectionKeys: [heading] });
        expect(found.hits.length).toBeGreaterThan(0);
        const grep = await turn.call("knowledge_grep", { scopeId: turn.scopeId, pattern, headingFilter: heading });
        expect(grep.matches).toHaveLength(1); expect(grep.matches[0].headingPath).toContain(heading);
        const quote = round === 1 ? "所有采购原则上必须提前审批。" : "紧急采购可以先行购买，并在两个工作日内补办审批。";
        expect((await turn.call("knowledge_research_update", { runId: turn.runId,
          linkEvidence: [{ needId, receiptId: grep.matches[0].receiptId, quote,
            relation: round === 1 ? "supports" : "contradicts", rationale: "不同章节的适用例外" }] })).isError).toBeUndefined();
      }
      expect((await requestFinish(turn)).accepted).toBe(false);
    });
    const output = await f.run(), checked = verifyFinalEvidence(f, output);
    expect(checked.run.status).toBe("partial"); expect(checked.needs[0].status).toBe("conflicted");
    expect(checked.texts).toEqual(expect.arrayContaining(["所有采购原则上必须提前审批。", "紧急采购可以先行购买，并在两个工作日内补办审批。"]));
    expect(f.invocations.filter(call => call.name === "knowledge_search").map(call => call.params.sectionKeys)).toEqual([["通用规则"], ["紧急例外"]]);
    expect(f.research.listEvidence(checked.run.id).map(item => item.headingPath?.[0]).sort()).toEqual(["通用规则", "紧急例外"].sort());
  });

  it("timeline：三个章节分给实际 Worker，三个时间点均由冻结原文支撑", async () => {
    const dates = ["2026年2月3日", "2026年5月12日", "2026年8月21日"];
    const headings = ["立项记录", "试运行记录", "上线记录"];
    const f = await fixture("timeline", async turn => {
      if (turn.role === "worker") {
        const need = f.research.getNeed(turn.runId, turn.options.research.allowedNeedIds[0]);
        const found = await search(turn, ["立项", "试运行", "上线"][need.ordinal], { sectionKeys: [headings[need.ordinal]] });
        expect(found.hits.length).toBeGreaterThan(0);
        expect((await readQualityQuote(turn, need.id, f.sources.history.sourceId, dates[need.ordinal],
          "supports", found.hits[0].chunkOrdinal + 1)).isError).toBeUndefined();
        return;
      }
      const needs = await createNeeds(turn, headings.map(claim => researchNeed(claim, { kind: "timeline" })));
      expect((await turn.call("knowledge_delegate", { runId: turn.runId,
        tasks: needs.map(need => ({ label: headings[need.ordinal], task: "独立定位这一阶段并读取原文", needIds: [need.id] })) })).isError).toBeUndefined();
      const finish = await requestFinish(turn);
      expect(finish.accepted, JSON.stringify({ finish, needs: f.research.listNeeds(turn.runId),
        actions: f.research.listActions(turn.runId).map(action => ({ name: action.actionType, status: action.status, error: action.errorCode })) })).toBe(true);
    });
    const output = await f.run(), checked = verifyFinalEvidence(f, output);
    expect(checked.run).toMatchObject({ status: "completed", delegatedAgents: 3 });
    expect(f.calls.filter(turn => turn.role === "worker")).toHaveLength(3);
    expect(checked.texts.sort()).toEqual([...dates].sort());
    expect(f.research.listEvidence(checked.run.id).map(item => item.headingPath?.[0]).sort()).toEqual([...headings].sort());
    expect(checked.needs.every(need => need.status === "supported")).toBe(true);
  });

  it("no-result：资料确无答案时保留缺口，不把空搜索写成事实或完整结论", async () => {
    let round = 0, needId = "";
    const f = await fixture("no-result", async turn => {
      if (++round === 1) needId = (await createNeeds(turn, [researchNeed("保险承保机构")]))[0].id;
      const found = await search(turn, round === 1 ? "承保机构" : "承保公司");
      expect(found.hits).toEqual([]);
      await turn.call("knowledge_research_update", { runId: turn.runId,
        unresolvedGaps: [{ needId, gaps: ["所选资料没有保险承保机构名称"] }] });
      expect((await requestFinish(turn)).accepted).toBe(false);
    });
    const output = await f.run(), checked = verifyFinalEvidence(f, output);
    expect(checked.run).toMatchObject({ status: "partial", stopReason: "no_progress", roundsCompleted: 2 });
    expect(checked.needs[0]).toMatchObject({ status: "uncovered", unresolvedGaps: ["所选资料没有保险承保机构名称"] });
    expect(checked.texts).toEqual([]); expect(checked.manifest.entries).toEqual([]);
    expect(output.block).toContain("所选资料没有保险承保机构名称");
  });

  it("scope-escape：答案真实存在于未选笔记本，搜索、读取和扫描均无法越界取正文", async () => {
    let round = 0;
    const f = await fixture("scope-escape", async turn => {
      if (++round === 1) {
        await createNeeds(turn, [researchNeed("协作会议入会码")]);
        for (const [tool, params] of [
          ["knowledge_search", { query: "入会码", sourceIds: [f.sources.secret.sourceId] }],
          ["knowledge_read", { sourceId: f.sources.secret.sourceId }],
          ["knowledge_grep", { pattern: "入会码", sourceIds: [f.sources.secret.sourceId] }],
        ] as const) expect(await turn.call(tool, { scopeId: turn.scopeId, ...params })).toMatchObject({ isError: true, errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
      }
      const found = await search(turn, round === 1 ? "入会码" : "青鹭");
      expect(found.hits.every((hit: { sourceId: string }) => hit.sourceId === f.sources.public.sourceId)).toBe(true);
      expect(JSON.stringify(found)).not.toContain("青鹭-4096");
      if (round === 2) expect(found.hits).toEqual([]);
      expect((await requestFinish(turn)).accepted).toBe(false);
    });
    const secretBlocks = f.manager.store.listArtifactBlocks({ studioId: f.request.compiledScope.studioId, parseArtifactId: f.sources.secret.parseArtifactId });
    expect(secretBlocks.some(block => block.text.includes("青鹭-4096"))).toBe(true);
    const readBlocks = vi.spyOn(f.manager.store, "getArtifactBlocksByIds");
    const listBlocks = vi.spyOn(f.manager.store, "listArtifactBlocks");
    const output = await f.run(), checked = verifyFinalEvidence(f, output);
    expect(checked.run).toMatchObject({ status: "partial", stopReason: "no_progress" });
    expect(f.invocations.filter(call => call.result.errorCode === "KNOWLEDGE_SCOPE_VIOLATION")).toHaveLength(3);
    expect(readBlocks.mock.calls.some(([input]) => input.parseArtifactId === f.sources.secret.parseArtifactId)).toBe(false);
    expect(listBlocks.mock.calls.some(([input]) => input.parseArtifactId === f.sources.secret.parseArtifactId)).toBe(false);
    expect(output.block).not.toContain("青鹭-4096"); expect(checked.texts).toEqual([]); expect(checked.manifest.entries).toEqual([]);
    expect(f.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_read_receipts WHERE run_id = ?").get(checked.run.id).count).toBe(0);
  });

  it("实际达到共享工具预算后输出 partial，不能由普通完成声明改成 completed", async () => {
    const f = await fixture("no-result", async turn => {
      await createNeeds(turn, [researchNeed("保险承保机构")]);
      for (let i = 0; i < DEFAULT_KNOWLEDGE_RESEARCH_BUDGET.maxToolCalls + 1; i++) {
        await turn.call("knowledge_outline", { scopeId: turn.scopeId });
      }
      return { stopReason: "stop", replyText: "已经全部完成" };
    });
    const output = await f.run(), checked = verifyFinalEvidence(f, output);
    expect(checked.run).toMatchObject({ status: "partial", stopReason: "tool_budget_exhausted", toolCallsUsed: 32 });
    expect(f.research.listActions(checked.run.id)).toHaveLength(DEFAULT_KNOWLEDGE_RESEARCH_BUDGET.maxToolCalls);
    expect(checked.needs[0].status).toBe("uncovered"); expect(checked.manifest.entries).toEqual([]);
    expect(output.block).not.toContain("已经全部完成");
  });
});
