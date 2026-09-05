import crypto from "node:crypto";
import { afterEach, describe, expect, it, vi } from "vitest";
import { EvidenceReceiptService } from "../lib/knowledge/evidence-receipt-service.ts";
import { ResearchStore } from "../lib/knowledge/research/research-store.ts";
import { createKnowledgeReadTool } from "../lib/tools/knowledge-read-tool.ts";
import { createKnowledgeGrepTool } from "../lib/tools/knowledge-grep-tool.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";
import { searchToolFixture } from "./helpers/knowledge-search-tool-fixture.ts";

const cleanups: Array<() => void | Promise<void>> = [];
afterEach(async () => {
  vi.restoreAllMocks();
  for (const close of cleanups.splice(0).reverse()) await close();
});

function setup(texts?: string[]) {
  const data = createKnowledgeResearchFixture(texts);
  cleanups.push(() => data.close());
  return data;
}
function issue(data: ReturnType<typeof setup>, overrides: Record<string, unknown> = {}) {
  const { text, ...source } = data.sources[0];
  return data.receipts.issue({ ...source, runId: data.run.id, actorSessionId: "reader-a",
    startOffset: 0, endOffset: text.length, channel: "knowledge_read", ...overrides });
}
function receiptCount(store: ReturnType<typeof setup>["store"]) {
  return store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_read_receipts").get().count as number;
}
function payload(result: { isError?: true; content: Array<{ text: string }> }) {
  expect(result.isError, result.content[0].text).not.toBe(true);
  return JSON.parse(result.content[0].text);
}

async function toolSetup(large = false) {
  const data = await searchToolFixture(large);
  cleanups.push(() => data.close());
  const research = new ResearchStore(data.manager.store);
  const run = research.createRun({ turnScopeId: data.scope.id, turnId: data.scope.turnId,
    parentSessionPath: data.scope.sessionPath, question: "制度是什么？" });
  const context = { runId: run.id, actorSessionId: "reader-a" };
  const deps = { getKnowledge: () => data.manager, getStudioId: () => data.studioId,
    resolveSessionContext: () => data.session };
  return { ...data, research, run, context, deps, receipts: new EvidenceReceiptService(research) };
}

describe("Research 冻结原文读取凭据", () => {
  it("只持久化位置与摘要，读回为冻结原文的精确切片", () => {
    const data = setup(["前缀：项目交付日期是九月十五日。后缀"]);
    const receipt = issue(data, { startOffset: 3, endOffset: 17 });
    const expected = data.sources[0].text.slice(3, 17);
    expect(receipt).toMatchObject({ runId: data.run.id, actorSessionId: "reader-a", startOffset: 3, endOffset: 17,
      canonicalTextSha256: crypto.createHash("sha256").update(expected).digest("hex"), consumedAt: null });
    expect(data.receipts.read({ runId: data.run.id, receiptId: receipt.id })).toMatchObject({
      receipt, block: { id: data.sources[0].blockId }, text: expected,
    });
    const row = data.store.db.prepare("SELECT * FROM knowledge_research_read_receipts WHERE id = ?").get(receipt.id);
    expect(Object.keys(row)).not.toContain("canonical_text");
    expect(JSON.stringify(row)).not.toContain(expected);
    expect(data.receipts.read({ runId: data.run.id, receiptId: receipt.id }).receipt.consumedAt).toBeNull();
  });

  it("不存在和跨运行的凭据、超出宿主来源子集以及错误工作会话身份均拒绝", () => {
    const data = setup();
    const receipt = issue(data);
    const other = data.research.createRun({ turnScopeId: data.scope.id, turnId: data.scope.turnId,
      parentSessionPath: data.scope.sessionPath, question: "另一项研究" });
    expect(() => data.receipts.read({ runId: data.run.id, receiptId: "search-snippet-id" })).toThrow();
    expect(() => data.receipts.read({ runId: other.id, receiptId: receipt.id })).toThrow();
    expect(() => data.receipts.read({ runId: data.run.id, receiptId: receipt.id, allowedSourceIds: [data.sources[1].sourceId] }))
      .toThrow(/scope/);
    expect(() => data.receipts.read({ runId: data.run.id, receiptId: receipt.id, actorSessionId: "reader-b" })).toThrow(/scope/);
    expect(() => issue(data, { allowedSourceIds: [] })).toThrow(/scope/);
    expect(() => issue(data, { allowedSourceIds: [data.sources[0].sourceId, "outside-source"] })).toThrow(/scope/);
    expect(receiptCount(data.store)).toBe(1);
  });

  it("原文变动即拒绝，连同原文块摘要一起篡改也不能替换旧凭据指向的文字", () => {
    const data = setup();
    const receipt = issue(data);
    const changed = data.sources[0].text.replace("九月", "十月");
    data.store.db.prepare("UPDATE knowledge_blocks SET text = ? WHERE id = ?").run(changed, data.sources[0].blockId);
    expect(() => data.receipts.read({ runId: data.run.id, receiptId: receipt.id })).toThrow(/hash/);
    expect(() => issue(data)).toThrow(/hash/);
    data.store.db.prepare("UPDATE knowledge_blocks SET text_sha256 = ? WHERE id = ?")
      .run(crypto.createHash("sha256").update(changed).digest("hex"), data.sources[0].blockId);
    expect(() => data.receipts.read({ runId: data.run.id, receiptId: receipt.id })).toThrow(/hash/);
  });

  it("关闭范围或结束研究后不能继续颁发或读取研究凭据", () => {
    const data = setup();
    const receipt = issue(data);
    for (const status of ["completed", "partial", "failed", "cancelled"]) {
      data.store.db.prepare("UPDATE knowledge_research_runs SET status = ? WHERE id = ?").run(status, data.run.id);
      expect(() => issue(data)).toThrow(/scope/);
      expect(() => data.receipts.read({ runId: data.run.id, receiptId: receipt.id })).toThrow(/scope/);
    }
    data.store.db.prepare("UPDATE knowledge_research_runs SET status = 'running' WHERE id = ?").run(data.run.id);
    data.store.closeTurnScope({ scopeId: data.scope.id });
    expect(() => issue(data)).toThrow(/scope/);
    expect(() => data.receipts.read({ runId: data.run.id, receiptId: receipt.id })).toThrow(/scope/);
  });

  it("摘要、解析产物和原文块不能跨来源拼接，偏移不能越界或指向空片段", () => {
    const data = setup();
    for (const field of ["sourceId", "contentSnapshotId", "parseArtifactId", "blockId"] as const) {
      expect(() => issue(data, { [field]: data.sources[1][field] })).toThrow();
    }
    for (const offsets of [
      { startOffset: -1 }, { startOffset: 0.5 }, { endOffset: 0 }, { endOffset: 1.5 },
      { startOffset: 2, endOffset: 2 }, { endOffset: data.sources[0].text.length + 1 },
    ]) expect(() => issue(data, offsets)).toThrow(/offset/);
    expect(() => issue(data, { channel: "knowledge_search" })).toThrow(/raw knowledge reads/);
    expect(receiptCount(data.store)).toBe(0);
  });

  it("冻结记录本身被拼错时也复核实际的来源、快照、解析产物身份链", () => {
    const data = setup();
    const first = data.sources[0], second = data.sources[1];
    data.store.db.prepare(`UPDATE knowledge_turn_scope_sources SET content_snapshot_id = ?, parse_artifact_id = ?
      WHERE scope_id = ? AND source_id = ?`).run(second.contentSnapshotId, second.parseArtifactId, data.scope.id, first.sourceId);
    expect(() => issue(data, { contentSnapshotId: second.contentSnapshotId, parseArtifactId: second.parseArtifactId, blockId: second.blockId }))
      .toThrow(/scope/);
    expect(receiptCount(data.store)).toBe(0);
  });

  it("同一批读取后段失败会回滚先前颁发的凭据", () => {
    const data = setup();
    expect(() => data.research.transaction(() => {
      issue(data);
      issue(data, { blockId: "missing-block" });
    })).toThrow();
    expect(receiptCount(data.store)).toBe(0);
  });
});

describe("Research knowledge_read 和 knowledge_grep 凭据接线", () => {
  it("普通读取、扫描和搜索提供引用链接，但不写研究凭据", async () => {
    const data = await toolSetup();
    const read = payload(await createKnowledgeReadTool(data.deps).execute("ordinary-read", { scopeId: data.scope.id, sourceId: data.source.id }));
    expect(read.spans[0]).not.toHaveProperty("receiptId");
    expect(read.spans[0].citationMarkdown).toContain("](");
    const grep = payload(await createKnowledgeGrepTool(data.deps).execute("ordinary-grep", { scopeId: data.scope.id, pattern: "needle" }));
    expect(grep.matches[0]).not.toHaveProperty("receiptId");
    const search = payload(await data.makeTool().execute("search", data.params));
    expect(search.hits[0]).not.toHaveProperty("receiptId");
    expect(receiptCount(data.manager.store)).toBe(0);
  });

  it("按序号读取只为返回的原文片段创建凭据，宿主身份不能由模型参数覆盖", async () => {
    const data = await toolSetup();
    const tool = createKnowledgeReadTool({ ...data.deps, resolveResearchContext: () => data.context });
    const read = payload(await tool.execute("read", { scopeId: data.scope.id, sourceId: data.source.id,
      runId: "invented-run", actorSessionId: "invented-actor" }));
    expect(read.chunks[0].spans).not.toHaveLength(0);
    for (const chunk of read.chunks) {
      for (const span of chunk.spans) {
        expect(data.receipts.read({ runId: data.run.id, receiptId: span.receiptId })).toMatchObject({
          receipt: { actorSessionId: "reader-a", channel: "knowledge_read", blockId: span.blockId }, text: span.text,
        });
      }
    }
    expect(receiptCount(data.manager.store)).toBe(read.chunks.reduce((count: number, chunk: { spans: unknown[] }) => count + chunk.spans.length, 0));
  });

  it("查询命中必须回读冻结原文，不能把检索摘要或索引缓存文字直接记为证据", async () => {
    const data = await toolSetup();
    const original = data.manager.searchService.searchWithEvidence.bind(data.manager.searchService);
    vi.spyOn(data.manager.searchService, "searchWithEvidence").mockImplementation(async (...args) => {
      const result = await original(...args);
      return { response: { ...result.response, hits: result.response.hits.map(hit => ({ ...hit, snippet: "伪造摘要" })) },
        evidence: { ...result.evidence, candidates: result.evidence.candidates.map(candidate => ({ ...candidate, text: "伪造索引缓存正文" })) } };
    });
    const read = payload(await createKnowledgeReadTool({ ...data.deps, resolveResearchContext: () => data.context })
      .execute("research-query", { scopeId: data.scope.id, sourceId: data.source.id, query: "needle" }));
    expect(read.matches.length).toBeGreaterThan(0);
    expect(JSON.stringify(read)).not.toContain("伪造");
    for (const match of read.matches) {
      expect(match.text).toContain("年假规定");
      for (const span of match.spans) expect(data.receipts.read({ runId: data.run.id, receiptId: span.receiptId }).text).toBe(span.text);
    }
  });

  it("扫描只给实际返回的有界上下文片段发凭据，未展示命中不能获得凭据", async () => {
    const data = await toolSetup(true);
    const grep = payload(await createKnowledgeGrepTool({ ...data.deps, resolveResearchContext: () => data.context })
      .execute("grep", { scopeId: data.scope.id, pattern: "needle", maxResults: 1 }));
    expect(grep.totalMatches).toBeGreaterThan(1);
    expect(grep.matches).toHaveLength(1);
    expect(receiptCount(data.manager.store)).toBe(1);
    const match = grep.matches[0];
    const { receipt, text } = data.receipts.read({ runId: data.run.id, receiptId: match.receiptId });
    expect(receipt).toMatchObject({ channel: "knowledge_grep", startOffset: match.receiptStartOffset, endOffset: match.receiptEndOffset });
    expect(text.length).toBeLessThanOrEqual(360);
    expect(match.snippet).toContain(text);
  });

  it("宿主缩小的来源范围在读取和扫描开始前生效", async () => {
    const data = await toolSetup();
    const deps = { ...data.deps, resolveResearchContext: () => ({ ...data.context, allowedSourceIds: [] }) };
    const read = await createKnowledgeReadTool(deps).execute("read", { scopeId: data.scope.id, sourceId: data.source.id });
    expect(read.isError).toBe(true);
    expect(read.details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    const grep = payload(await createKnowledgeGrepTool(deps).execute("grep", { scopeId: data.scope.id, pattern: "needle" }));
    expect(grep.scannedSources).toEqual([]);
    expect(grep.matches).toEqual([]);
    const forbidden = await createKnowledgeGrepTool(deps).execute("grep-explicit", {
      scopeId: data.scope.id, pattern: "needle", sourceIds: [data.source.id],
    });
    expect(forbidden.isError).toBe(true);
    expect(forbidden.details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(receiptCount(data.manager.store)).toBe(0);
  });

  it("无命中或已取消的读取不颁发凭据", async () => {
    const data = await toolSetup();
    const deps = { ...data.deps, resolveResearchContext: () => data.context };
    const grep = payload(await createKnowledgeGrepTool(deps).execute("grep-empty", { scopeId: data.scope.id, pattern: "没有这个词" }));
    expect(grep.matches).toEqual([]);
    const aborted = new AbortController(); aborted.abort();
    await expect(createKnowledgeReadTool(deps).execute("read-cancelled", { scopeId: data.scope.id, sourceId: data.source.id }, aborted.signal)).rejects.toBe(aborted.signal.reason);
    expect(receiptCount(data.manager.store)).toBe(0);
  });
});
