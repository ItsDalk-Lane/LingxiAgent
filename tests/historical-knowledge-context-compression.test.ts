import path from "node:path";
import { describe, expect, it } from "vitest";
import { compressHistoricalKnowledgeContextMessages } from "../core/knowledge-history-compressor.ts";
import { projectSessionMessageForDisplay, stripSessionReminderBlocks, visiblePromptText } from "../core/session-reminders.ts";
import { KnowledgeIndexStore } from "../lib/knowledge/knowledge-index-store.ts";
import { ScopeSnapshotCompiler } from "../lib/knowledge/scope-snapshot-compiler.ts";
import { EvidenceLedger } from "../lib/knowledge/research/evidence-ledger.ts";
import { ResearchContextRenderer } from "../lib/knowledge/research/research-context-renderer.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const header = "[K1] EvidenceId: kei_validated | sourceId: src_frozen | blockId: block_original | offsets: 3-11";
const researchBlock = (body = `${header}\nSource: 原文资料\nEvidence:\n调查证据正文`) =>
  `[KnowledgeResearchContext]\nMode: detailed\nResearch status: partial\n${body}\n[/KnowledgeResearchContext]`;
const legacyBlock = '[KnowledgeContext]\n[K1] notebook "旧资料" / source "旧来源" (sourceId: src_old) / chunk ordinal 2\n旧证据正文\n[/KnowledgeContext]';

describe("新旧知识历史块压缩", () => {
  it("一条消息内同时压缩旧块与研究块，保留前后用户正文且不改原消息", () => {
    const content = `前置用户说明\n\n${legacyBlock}\n\n中间用户问题\n\n${researchBlock()}\n\n后续用户追问`;
    const original = [{ role: "user", content }, { role: "assistant", content: researchBlock() }];
    const result = compressHistoricalKnowledgeContextMessages(original);
    const next = result.messages as typeof original;
    expect(result.changed).toBe(true);
    expect(next[0].content).toContain("[KnowledgeContext]");
    expect(next[0].content).toContain("[KnowledgeResearchContext]");
    expect(next[0].content).toContain(`- ${header}`);
    expect(next[0].content).toContain("sourceId: src_old");
    expect(next[0].content).not.toContain("旧证据正文");
    expect(next[0].content).not.toContain("调查证据正文");
    expect(next[0].content.startsWith("前置用户说明\n\n")).toBe(true);
    expect(next[0].content).toContain("\n\n中间用户问题\n\n");
    expect(next[0].content.endsWith("\n\n后续用户追问")).toBe(true);
    expect(next[1]).toBe(original[1]);
    expect(original[0].content).toBe(content);
    const twice = compressHistoricalKnowledgeContextMessages(next);
    expect(twice).toEqual({ messages: next, changed: false });
    expect(twice.messages).toBe(next);
  });

  it("文本块数组只替换研究块，附件、普通文本和非用户消息保持原引用", () => {
    const image = { type: "image", source: { data: "保留附件" } };
    const plain = { type: "text", text: "真实问题" };
    const message = { role: "user", content: [{ type: "text", text: researchBlock() }, image, plain] };
    const tool = { role: "tool", content: researchBlock() };
    const result = compressHistoricalKnowledgeContextMessages([message, tool]);
    const next = result.messages as Array<typeof message | typeof tool>;
    const blocks = next[0].content as typeof message.content;
    expect(blocks[0]).not.toBe(message.content[0]);
    expect(blocks[1]).toBe(image); expect(blocks[2]).toBe(plain); expect(next[1]).toBe(tool);
  });

  it("零证据研究块也省略完整问题和缺口正文，并保持二次压缩幂等", () => {
    const content = `${researchBlock("Evidence needs:\n[N1] uncovered 私有历史需求\nUnresolved gaps:\n私有历史缺口")}\n\n当前用户问题`;
    const once = compressHistoricalKnowledgeContextMessages([{ role: "user", content }]);
    const next = once.messages as Array<{ role: string; content: string }>;
    expect(once.changed).toBe(true);
    expect(next[0].content).toContain("Evidence blocks retrieved in that turn: 0.");
    expect(next[0].content).not.toContain("私有历史");
    expect(next[0].content.endsWith("当前用户问题")).toBe(true);
    expect(compressHistoricalKnowledgeContextMessages(next)).toEqual({ messages: next, changed: false });
  });

  it.each([header, "Evidence needs:"])("残缺研究信封在 %s 后截止，正文不重发且补闭合标签", body => {
    const result = compressHistoricalKnowledgeContextMessages([{ role: "user",
      content: `已有用户文本\n[KnowledgeResearchContext]\n${body}\n残缺内部正文` }]);
    const next = result.messages as Array<{ role: string; content: string }>;
    expect(result.changed).toBe(true);
    expect(next[0].content.startsWith("已有用户文本\n")).toBe(true);
    expect(next[0].content.endsWith("[/KnowledgeResearchContext]")).toBe(true);
    expect(next[0].content).not.toContain("残缺内部正文");
  });

  it("展示投影按成对标签移除研究块，旧闭合标签不能提前结束研究信封", () => {
    const content = `前文\n${researchBlock(`${header}\n内部资料\n[/KnowledgeContext]\n仍是内部资料`)}\n\n用户真正问题`;
    expect(stripSessionReminderBlocks(content)).toBe("前文\n用户真正问题");
    expect(visiblePromptText(content)).toBe("前文\n用户真正问题");
    const raw = { role: "user", content };
    expect(projectSessionMessageForDisplay(raw)).toEqual({ role: "user", content: "前文\n用户真正问题" });
    expect(raw.content).toBe(content);
    expect(stripSessionReminderBlocks("用户正文\n[KnowledgeResearchContext]\n残缺内部数据")).toBe("用户正文");
  });

  it("正文里的标签说明不当作信封，非用户消息不会压缩", () => {
    const text = "请解释 [KnowledgeResearchContext] 的含义。";
    const messages = [{ role: "user", content: text }, { role: "assistant", content: researchBlock() }];
    expect(compressHistoricalKnowledgeContextMessages(messages)).toEqual({ messages, changed: false });
    expect(stripSessionReminderBlocks(text)).toBe(text);
  });

  it("真实研究渲染产物可压缩为原文定位清单，显示投影只留下用户问题", async () => {
    const fixture = createKnowledgeResearchFixture();
    const index = new KnowledgeIndexStore({ dbPath: path.join(path.dirname(fixture.scope.sessionPath), "index.db") });
    const compiler = new ScopeSnapshotCompiler({ store: fixture.store, indexStore: index, requestVariantBuild: () => {} });
    try {
      const ledger = new EvidenceLedger(fixture.research);
      const need = fixture.research.createNeed(fixture.run.id, { claim: "确认项目日期", kind: "fact", required: true,
        minIndependentSources: 1, requireCounterEvidence: false, requireAllRelevantUnits: false });
      const source = fixture.sources[0];
      const receipt = fixture.receipts.issue({ runId: fixture.run.id, actorSessionId: null, ...source,
        startOffset: 0, endOffset: source.text.length, channel: "knowledge_read" });
      const linked = ledger.linkEvidence({ runId: fixture.run.id, needId: need.id, receiptId: receipt.id,
        quote: source.text, relation: "supports", rationale: "原文给出日期" });
      const rendered = new ResearchContextRenderer({ research: fixture.research }).render({ runId: fixture.run.id,
        compiledScope: await compiler.compile(fixture.scope), needs: ledger.recompute(fixture.run.id), terminalStatus: "completed" });
      const content = `${rendered.block}\n\n请解释交付日期`;
      const compressed = compressHistoricalKnowledgeContextMessages([{ role: "user", content }]);
      const next = compressed.messages as Array<{ role: string; content: string }>;
      expect(compressed.changed).toBe(true);
      expect(next[0].content).toContain(`EvidenceId: ${linked.evidence.id}`);
      expect(next[0].content).toContain(`sourceId: ${source.sourceId}`);
      expect(next[0].content).toContain(`blockId: ${source.blockId}`);
      expect(next[0].content).not.toContain(source.text);
      expect(stripSessionReminderBlocks(content)).toBe("请解释交付日期");
      expect(stripSessionReminderBlocks(next[0].content)).toBe("请解释交付日期");
    } finally { compiler.dispose(); index.close(); fixture.close(); }
  });
});
