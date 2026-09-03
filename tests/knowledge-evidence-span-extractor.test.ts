import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { evidencePrefixWithinBudget } from "../lib/knowledge/evidence-span-extractor.ts";
import { estimateTextTokens } from "../lib/llm/estimate-text-tokens.ts";
import type { KnowledgeBlockDraft } from "../lib/knowledge/source-adapters.ts";
import { resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";

const managers: KnowledgeManager[] = [];
const homes: string[] = [];
const studioId = "span-studio";
afterEach(() => {
  vi.restoreAllMocks();
  for (const manager of managers.splice(0)) manager.close();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});

async function fixture(texts: string[], locator: Record<string, unknown> = {}, locatorType: KnowledgeBlockDraft["locatorType"] = "text") {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-evidence-span-"));
  homes.push(home);
  const manager = new KnowledgeManager({ lingxiHome: home });
  managers.push(manager);
  const notebook = manager.createNotebook({ studioId, name: "原文" });
  manager.updateNotebookSettings({ studioId, notebookId: notebook.id, chunkTargetChars: 100_000 });
  const imported = await manager.importPastedText({ studioId, notebookId: notebook.id, text: texts.join("\n"), displayName: "原始资料" });
  const artifact = manager.store.beginParseArtifact({ studioId, contentSnapshotId: imported.snapshot.id,
    parserId: "span-fixture", parserVersion: "1", parserConfigHash: "a".repeat(64) });
  manager.store.completeParseArtifact({ studioId, parseArtifactId: artifact.id, status: "ready", warnings: [],
    semanticArtifactPath: `artifacts/${artifact.id}.json`,
    blocks: texts.map((text, ordinal) => ({ ordinal, text, locatorType, locator })),
  });
  // 这里检验提取器，保留上面明确给定的解析块；不让文本解析器重新拆段或覆盖页码。
  const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
  const config = resolveKnowledgeChunkerConfig(blocks, { targetChars: 100_000 });
  manager.store.resolveNotebookRetrievalProfile({ studioId, notebookId: notebook.id, strategy: config.strategy });
  manager.queryService.indexArtifactForIngestion(studioId, artifact.id, { targetChars: 100_000 });
  const scope = manager.createTurnScope({ studioId, sessionPath: "/tmp/span-session.jsonl", notebookIds: [notebook.id] });
  const compiledScope = await manager.compileTurnScope(scope);
  return { manager, compiledScope, blocks, artifact };
}

async function extract(texts: string[], query: string, locator: Record<string, unknown> = {}, locatorType: KnowledgeBlockDraft["locatorType"] = "text") {
  const result = await fixture(texts, locator, locatorType);
  const hits = result.manager.queryService.searchCompiledScopeFts({ compiledScope: result.compiledScope, query, limit: 24 });
  const spans = result.manager.queryService.extractEvidenceSpans({ compiledScope: result.compiledScope, hits, query });
  return { ...result, hits, spans };
}

describe("精确证据范围", () => {
  it.each([
    ["员工每年享有十五天年假。", "年假"],
    ["The release policy requires two approvals.", "release policy"],
    ["请将 retry_count 配置为 12345，写入 release.config.json。", "retry_count 12345 release.config.json"],
  ])("中文、英文、数字和配置键都从原文提取：%s", async (text, query) => {
    const { spans, blocks } = await extract([text], query);
    expect(spans.length).toBeGreaterThan(0);
    expect(spans[0].text).toBe(text);
    expect(spans[0].text).toBe(blocks[0].text.slice(spans[0].startOffset, spans[0].endOffset));
    expect(spans[0].textSha256).toBe(crypto.createHash("sha256").update(text).digest("hex"));
  });

  it("一个命中跨多个原始块时分别回读原文，不包含合成面包屑", async () => {
    const { spans, blocks } = await extract(["苹果版本九月发布。", "香蕉版本十月发布。"], "版本发布", { headingPath: ["合成标题", "版本"] }, "markdown");
    expect(spans).toHaveLength(2);
    for (const span of spans) {
      expect(span.text).toBe(blocks.find(block => block.id === span.blockId)!.text);
      expect(span.text).not.toContain("合成标题");
    }
  });

  it("最长合法前缀和长证据窗口均不会破坏 Unicode 代理对", async () => {
    expect(evidencePrefixWithinBudget("abc😀xyz", 1)).toBe("abc😀");
    expect(evidencePrefixWithinBudget("abc𠮷xyz", 1)).toBe("abc");
    const text = "😀背景".repeat(500) + "关键结果是新版本发布。" + "𠮷结束".repeat(500);
    const { spans, blocks } = await extract([text], "关键结果 新版本发布");
    expect(spans[0].text).toContain("关键结果");
    expect(spans[0].text.isWellFormed()).toBe(true);
    expect(spans[0].text).toBe(blocks[0].text.slice(spans[0].startOffset, spans[0].endOffset));
    expect(estimateTextTokens(spans[0].text)).toBeLessThanOrEqual(320);
  });

  it("优先保留包含密集查询词的完整段落", async () => {
    const target = "苹果发布计划已经批准。香蕉发布计划需要延期。苹果和香蕉共用发布流程。";
    const text = "无关背景。".repeat(100) + "\n\n" + target + "\n\n" + "其他材料。".repeat(100);
    const { spans } = await extract([text], "苹果 香蕉 发布计划");
    expect(spans[0].text).toBe(target);
    expect(spans[0].startOffset).toBe(text.indexOf(target));
    expect(spans[0].endOffset).toBe(text.indexOf(target) + target.length);
  });

  it("长段落退到完整句子边界，保留命中位置", async () => {
    const text = "背景说明内容很多。".repeat(80) + "批准发布需要两位负责人同意。" + "后续归档要求明确。".repeat(80);
    const { spans } = await extract([text], "批准发布 两位负责人");
    expect(spans[0].text).toContain("批准发布需要两位负责人同意。");
    expect(spans[0].text.endsWith("。")).toBe(true);
    expect(spans[0].startOffset === 0 || text[spans[0].startOffset - 1] === "。").toBe(true);
    expect(estimateTextTokens(spans[0].text)).toBeLessThanOrEqual(320);
  });

  it("没有自然边界时仍严格限制 token 并保留精确偏移", async () => {
    const text = "背景".repeat(500) + "核心配置retry_count=12345" + "背景".repeat(500);
    const { spans, blocks } = await extract([text], "核心配置 retry_count");
    expect(spans[0].text).toContain("核心配置");
    expect(estimateTextTokens(spans[0].text)).toBeLessThanOrEqual(320);
    expect(spans[0].text).toBe(blocks[0].text.slice(spans[0].startOffset, spans[0].endOffset));
  });

  it("同块重叠超过六成时保留得分较高的范围", async () => {
    const { manager, compiledScope } = await fixture(["发布计划要求审批后再上线。"]);
    const hit = manager.queryService.searchCompiledScopeFts({ compiledScope, query: "发布", limit: 24 })[0];
    const spans = manager.queryService.extractEvidenceSpans({ compiledScope, query: "发布",
      hits: [{ ...hit, score: -1 }, { ...hit, score: -10 }],
    });
    expect(spans).toHaveLength(1);
    expect(spans[0].score).toBe(10);
  });

  it("相同产物的多条命中只执行一次批量回读", async () => {
    const { manager, compiledScope } = await fixture(["发布计划要求审批后再上线。", "发布前还要确认备份。"]);
    const hits = manager.queryService.searchCompiledScopeFts({ compiledScope, query: "发布", limit: 24 });
    const batch = vi.spyOn(manager.store, "getArtifactBlocksByIds");
    const full = vi.spyOn(manager.store, "listArtifactBlocks").mockImplementation(() => { throw new Error("不得读取全部原文"); });
    manager.queryService.extractEvidenceSpans({ compiledScope, query: "发布", hits: [...hits, ...hits] });
    expect(batch).toHaveBeenCalledTimes(1);
    expect(full).not.toHaveBeenCalled();
  });

  it("保留章节路径与 PDF 页码", async () => {
    const { spans } = await extract(["发布前需要审批。"], "发布", { headingPath: ["第一章", "发布"], pageNumber: 7 }, "pdf");
    expect(spans[0].headingPath).toEqual(["第一章", "发布"]);
    expect(spans[0].pageNumber).toBe(7);
  });

  it("批量原文读取不能跨工作室，伪造范围或损坏定位被拒绝", async () => {
    const { manager, compiledScope, blocks, artifact } = await fixture(["发布前需要审批。"]);
    expect(manager.store.getArtifactBlocksByIds({ studioId: "other", parseArtifactId: artifact.id, blockIds: [blocks[0].id] })).toEqual([]);
    const hit = manager.queryService.searchCompiledScopeFts({ compiledScope, query: "发布", limit: 24 })[0];
    expect(() => manager.queryService.extractEvidenceSpans({ compiledScope, query: "发布", hits: [{ ...hit, chunkIndexVariantId: "unknown" }] }))
      .toThrow(expect.objectContaining({ code: "KNOWLEDGE_SCOPE_VIOLATION" }));
    expect(() => manager.queryService.extractEvidenceSpans({ compiledScope, query: "发布", hits: [{ ...hit,
      spans: [{ ...hit.spans[0], blockEndOffset: 99999 }],
    }] })).toThrow(expect.objectContaining({ code: "KNOWLEDGE_INDEX_INVALID" }));
  });
});
