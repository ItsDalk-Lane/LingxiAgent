import crypto from "node:crypto";
import { afterEach, describe, expect, it, vi } from "vitest";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const fixtures: ReturnType<typeof createKnowledgeResearchFixture>[] = [];
afterEach(() => { for (const fixture of fixtures.splice(0)) fixture.close(); });

function setup(text = "开头😀。交付九月十五日。结束。交付九月十五日。") {
  const fixture = createKnowledgeResearchFixture([text]);
  fixtures.push(fixture);
  const { research, receipts, run, sources } = fixture;
  const need = research.createNeed(run.id, { claim: "确定交付时间", kind: "fact", required: true,
    minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false });
  const receipt = receipts.issue({ runId: run.id, actorSessionId: "worker-1", ...sources[0],
    startOffset: 0, endOffset: text.length, channel: "knowledge_read" });
  return { ...fixture, need, receipt, ledger: new EvidenceLedger(research),
    input: { runId: run.id, needId: need.id, receiptId: receipt.id,
      quote: "交付九月十五日", relation: "supports" as const, rationale: "原文明确给出日期" } };
}

describe("研究证据精确引文校验", () => {
  it("拒绝伪造文字、搜索摘要和用分块编号冒充阅读凭据", () => {
    const f = setup();
    expect(() => f.ledger.linkEvidence({ ...f.input, quote: "交付九月二十日" })).toThrow(/does not occur/);
    expect(() => f.ledger.linkEvidence({ ...f.input, receiptId: f.sources[0].blockId })).toThrow();
    expect(f.research.listEvidence(f.run.id)).toEqual([]);
    expect(f.research.getReceipt(f.run.id, f.receipt.id).consumedAt).toBeNull();
  });

  it("重复原文必须指定出现次数，按零基位置推导真实偏移", () => {
    const f = setup();
    expect(() => f.ledger.linkEvidence(f.input)).toThrow(/occurrenceIndex/);
    for (const occurrenceIndex of [-1, 0.5, 2, NaN]) {
      expect(() => f.ledger.linkEvidence({ ...f.input, occurrenceIndex })).toThrow();
    }
    const first = f.ledger.linkEvidence({ ...f.input, occurrenceIndex: 0 });
    const second = f.ledger.linkEvidence({ ...f.input, occurrenceIndex: 1 });
    expect(first.evidence.startOffset).toBe(f.sources[0].text.indexOf(f.input.quote));
    expect(second.evidence.startOffset).toBe(f.sources[0].text.lastIndexOf(f.input.quote));
    expect(second.evidence.endOffset - second.evidence.startOffset).toBe(f.input.quote.length);
    expect(second.evidence.canonicalTextSha256).toBe(crypto.createHash("sha256").update(f.input.quote).digest("hex"));
    expect(second.evidence.headingPath).toEqual(["项目"]);
    expect(f.research.listEvidence(f.run.id)).toHaveLength(2);
    expect(f.research.getReceipt(f.run.id, f.receipt.id).consumedAt).not.toBeNull();
  });

  it("凭据从块中间起始时保留真实绝对偏移，重叠匹配也必须消歧", () => {
    const f = setup("前言aaaaa尾部");
    const receipt = f.receipts.issue({ runId: f.run.id, actorSessionId: null, ...f.sources[0],
      startOffset: 2, endOffset: 7, channel: "knowledge_grep" });
    const result = f.ledger.linkEvidence({ ...f.input, receiptId: receipt.id, quote: "aaa", occurrenceIndex: 2 });
    expect(result.evidence.startOffset).toBe(4);
    expect(result.evidence.endOffset).toBe(7);
  });

  it("严格保留空格和换行，不能用近似引文代替原文", () => {
    const f = setup("交付\n九月  十五日");
    expect(() => f.ledger.linkEvidence({ ...f.input, quote: "交付九月十五日" })).toThrow();
    const result = f.ledger.linkEvidence({ ...f.input, quote: f.sources[0].text });
    expect(result.evidence.canonicalText).toBe(f.sources[0].text);
  });

  it("引文最多2000字符，空引文与超长理由均拒绝，边界2000可入账", () => {
    const f = setup("字".repeat(2001));
    for (const quote of ["", " ", "字".repeat(2001)]) {
      expect(() => f.ledger.linkEvidence({ ...f.input, quote })).toThrow();
    }
    expect(() => f.ledger.linkEvidence({ ...f.input, quote: "字", occurrenceIndex: 0, rationale: "理".repeat(1001) })).toThrow();
    const result = f.ledger.linkEvidence({ ...f.input, quote: "字".repeat(2000), occurrenceIndex: 0 });
    expect(result.evidence.canonicalText).toHaveLength(2000);
  });

  it("同一真实span再次关联复用证据，不能重复计入来源或证据数量", () => {
    const f = setup("交付九月十五日。");
    const first = f.ledger.linkEvidence(f.input);
    const second = f.ledger.linkEvidence(f.input);
    expect(second.evidence.id).toBe(first.evidence.id);
    expect(f.research.listEvidence(f.run.id)).toHaveLength(1);
    expect(f.research.listRelations(f.run.id, f.need.id)).toHaveLength(1);
    expect(second.need.independentSourceCount).toBe(1);
  });

  it("跨运行凭据、越工作分配的需求或来源均不得入账", () => {
    const f = setup("交付九月十五日。");
    const other = f.research.createRun({ turnScopeId: f.scope.id, turnId: f.scope.turnId,
      parentSessionPath: f.scope.sessionPath, question: "另一个研究问题" });
    const need = f.research.createNeed(other.id, { claim: "另一个需求", kind: "fact", required: true,
      minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false });
    expect(() => f.ledger.linkEvidence({ ...f.input, runId: other.id, needId: need.id })).toThrow();
    expect(() => f.ledger.linkEvidence(f.input, { allowedNeedIds: [] })).toThrow();
    expect(() => f.ledger.linkEvidence(f.input, { allowedSourceIds: [] })).toThrow();
    expect(f.research.listEvidence(f.run.id)).toEqual([]);
  });

  it("冻结原文hash漂移和receipt hash漂移都拒绝，不能通过一起重写块hash伪造已读内容", () => {
    const f = setup("交付九月十五日。");
    f.store.db.prepare("UPDATE knowledge_blocks SET text = ? WHERE id = ?").run("交付九月二十日。", f.sources[0].blockId);
    expect(() => f.ledger.linkEvidence(f.input)).toThrow(/hash/);
    const changed = crypto.createHash("sha256").update("交付九月二十日。").digest("hex");
    f.store.db.prepare("UPDATE knowledge_blocks SET text_sha256 = ? WHERE id = ?").run(changed, f.sources[0].blockId);
    expect(() => f.ledger.linkEvidence({ ...f.input, quote: "交付九月二十日" })).toThrow(/hash/);
    expect(f.research.listEvidence(f.run.id)).toEqual([]);
  });

  it("消费凭据之后的状态写入失败会回滚证据、关联和消费，原凭据可重试", () => {
    const f = setup("交付九月十五日。");
    const failure = vi.spyOn(f.research, "setNeedState").mockImplementationOnce(() => { throw new Error("实际事务中断"); });
    expect(() => f.ledger.linkEvidence(f.input)).toThrow("实际事务中断");
    failure.mockRestore();
    expect(f.research.listEvidence(f.run.id)).toEqual([]);
    expect(f.research.listRelations(f.run.id, f.need.id)).toEqual([]);
    expect(f.research.getReceipt(f.run.id, f.receipt.id).consumedAt).toBeNull();
    expect(f.ledger.linkEvidence(f.input).need.status).toBe("supported");
  });
});
