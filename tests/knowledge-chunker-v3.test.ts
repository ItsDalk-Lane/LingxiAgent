import crypto from "node:crypto";
import { describe, expect, it } from "vitest";
import {
  buildKnowledgeChunks, buildKnowledgeSections, buildLegacyKnowledgeChunks, computeAutoChunkTargetChars,
  knowledgeBlockFingerprint, knowledgeChunkerConfigId, legacyKnowledgeBlockFingerprint, legacyKnowledgeChunkerConfigId,
  resolveKnowledgeChunkerConfig, resolveLegacyKnowledgeChunkerConfig,
  KNOWLEDGE_CHUNKER_VERSION, KNOWLEDGE_CHUNK_TARGET_CHARS, KNOWLEDGE_SPAN_TARGET_TOKENS,
  KNOWLEDGE_SPAN_OVERLAP_TOKENS, KNOWLEDGE_SECTION_SOFT_MAX_TOKENS,
  type KnowledgeChunkDraft, type KnowledgeSectionDraft,
} from "../lib/knowledge/chunker.ts";
import { estimateTextTokens } from "../lib/llm/estimate-text-tokens.ts";
import type { KnowledgeBlock } from "../lib/knowledge/types.ts";

const artifactId = "parse-v3";
function block(ordinal: number, text: string, locatorType: KnowledgeBlock["locatorType"] = "text", locator: Record<string, unknown> = {}): KnowledgeBlock {
  return { id: `block-${ordinal}`, parseArtifactId: artifactId, ordinal, text,
    textSha256: crypto.createHash("sha256").update(text).digest("hex"), locatorType, locator };
}
function expectSectionsComplete(blocks: KnowledgeBlock[], sections: KnowledgeSectionDraft[]) {
  expect(sections.flatMap(section => section.primaryBlockIds ?? [])).toEqual([...blocks].sort((a, b) => a.ordinal - b.ordinal).map(item => item.id));
  for (const [ordinal, section] of sections.entries()) {
    expect(section.sectionOrdinal).toBe(ordinal);
    expect(section.tokenCount).toBe(estimateTextTokens(section.text));
    expect(section.tokenCount).toBeLessThanOrEqual(8192);
  }
  for (const original of blocks) {
    const pieces = sections.flatMap(section => (section.blockSpans ?? []).filter(span => span.blockId === original.id).map(span => ({ section, span })));
    let cursor = 0;
    for (const { section, span } of pieces) {
      expect(span.blockStartOffset).toBe(cursor);
      expect(span.blockEndOffset).toBeGreaterThan(span.blockStartOffset);
      expect(section.text.slice(span.sectionStartOffset, span.sectionEndOffset)).toBe(original.text.slice(span.blockStartOffset, span.blockEndOffset));
      expect(section.startBlockOrdinal).toBeLessThanOrEqual(original.ordinal);
      expect(section.endBlockOrdinal).toBeGreaterThanOrEqual(original.ordinal);
      cursor = span.blockEndOffset;
    }
    expect(cursor).toBe(original.text.length);
  }
}
function expectChunksExact(blocks: KnowledgeBlock[], sections: KnowledgeSectionDraft[], chunks: KnowledgeChunkDraft[]) {
  const byId = new Map(blocks.map(item => [item.id, item]));
  const covered = new Map(blocks.map(item => [item.id, new Uint16Array(item.text.length)]));
  for (const [ordinal, chunk] of chunks.entries()) {
    expect(chunk.ordinal).toBe(ordinal);
    expect(chunk.tokenCount).toBe(estimateTextTokens(chunk.text));
    expect(chunk.tokenCount).toBeLessThanOrEqual(512);
    const section = sections.find(item => item.id === chunk.sectionId);
    expect(section).toBeDefined(); expect(chunk.spans.length).toBeGreaterThan(0);
    for (const span of chunk.spans) {
      const original = byId.get(span.blockId)!;
      expect(span.blockStartOffset).toBeGreaterThanOrEqual(0);
      expect(span.blockEndOffset).toBeLessThanOrEqual(original.text.length);
      expect(chunk.text.slice(span.chunkStartOffset, span.chunkEndOffset)).toBe(original.text.slice(span.blockStartOffset, span.blockEndOffset));
      expect(section!.blockSpans!.some(parent => parent.blockId === span.blockId
        && parent.blockStartOffset <= span.blockStartOffset && parent.blockEndOffset >= span.blockEndOffset)).toBe(true);
      for (let index = span.blockStartOffset; index < span.blockEndOffset; index++) covered.get(span.blockId)![index]++;
    }
  }
  for (const counts of covered.values()) expect([...counts].every(count => count >= 1)).toBe(true);
}

describe("v3章节和原文片段的固定粒度", () => {
  it("锁定版本与预算，固定默认不随历史大模型窗口放大", () => {
    expect([KNOWLEDGE_CHUNKER_VERSION, KNOWLEDGE_CHUNK_TARGET_CHARS, KNOWLEDGE_SPAN_TARGET_TOKENS,
      KNOWLEDGE_SPAN_OVERLAP_TOKENS, KNOWLEDGE_SECTION_SOFT_MAX_TOKENS]).toEqual(["3", 2048, 512, 64, 8192]);
    const blocks = [block(0, "大窗口不能扩大原文片段。".repeat(2500))];
    const normal = buildKnowledgeChunks(artifactId, blocks);
    const fromHugeLegacyWindow = buildKnowledgeChunks(artifactId, blocks, { targetChars: computeAutoChunkTargetChars(1_000_000) });
    expect(resolveKnowledgeChunkerConfig(blocks).targetChars).toBe(2048);
    expect(fromHugeLegacyWindow.map(chunk => chunk.text)).toEqual(normal.map(chunk => chunk.text));
    expect(fromHugeLegacyWindow.every(chunk => chunk.tokenCount <= 512)).toBe(true);
    expect(fromHugeLegacyWindow.map(chunk => chunk.id)).not.toEqual(normal.map(chunk => chunk.id));
  });

  it.each(["markdown", "html"] as const)("%s按完整标题路径切节，子标题不被合并到同一个顶层节", locatorType => {
    const blocks = [block(0, "前言", locatorType, { headingPath: [] }),
      block(1, "第一节正文", locatorType, { headingPath: ["指南", "安装"] }),
      block(2, "第一节续文", locatorType, { headingPath: ["指南", "安装"] }),
      block(3, "第二节正文", locatorType, { headingPath: ["指南", "配置"] })];
    const sections = buildKnowledgeSections(artifactId, blocks);
    expect(sections.map(section => section.headingPath)).toEqual([[], ["指南", "安装"], ["指南", "配置"]]);
    expect(sections.map(section => [section.startBlockOrdinal, section.endBlockOrdinal])).toEqual([[0, 0], [1, 2], [3, 3]]);
    expectSectionsComplete(blocks, sections); expectChunksExact(blocks, sections, buildKnowledgeChunks(artifactId, blocks));
  });

  it("章节文本识别中英文标题，现代入口不因前200块没有标题而漏掉后文章节", () => {
    const blocks = Array.from({ length: 210 }, (_, index) => block(index, `前言第${index}行。`));
    blocks.push(block(210, "第一章 风起"), block(211, "章节事实"), block(212, "Chapter 2"), block(213, "The next fact."));
    expect(resolveKnowledgeChunkerConfig(blocks).strategy).toBe("text");
    expect(resolveLegacyKnowledgeChunkerConfig(blocks).strategy).toBe("fixed");
    const sections = buildKnowledgeSections(artifactId, blocks);
    expect(sections.map(section => section.headingPath)).toEqual([[], ["第一章 风起"], ["Chapter 2"]]);
    expectSectionsComplete(blocks, sections); expectChunksExact(blocks, sections, buildKnowledgeChunks(artifactId, blocks));
  });

  it("PDF优先沿标题组织正文，没有标题的资料才按页面组织", () => {
    const headingBlocks = [block(0, "标题一", "pdf", { page: 1, headingPath: ["第一部分"] }),
      block(1, "跨页续文", "pdf", { page: 2 }), block(2, "标题二", "pdf", { page: 2, headingPath: ["第二部分"] }),
      block(3, "继续正文", "pdf", { page: 3 })];
    const headingSections = buildKnowledgeSections(artifactId, headingBlocks);
    expect(headingSections.map(section => [section.startBlockOrdinal, section.endBlockOrdinal])).toEqual([[0, 1], [2, 3]]);
    expect(headingSections.map(section => section.headingPath)).toEqual([["第一部分"], ["第二部分"]]);
    expectSectionsComplete(headingBlocks, headingSections);
    const pageBlocks = [block(0, "第一页甲", "pdf", { pageNumber: 1 }), block(1, "第一页乙", "pdf", { page: 1 }),
      block(2, "第二页", "pdf", { page: 2 })];
    const pageSections = buildKnowledgeSections(artifactId, pageBlocks);
    expect(pageSections.map(section => [section.startBlockOrdinal, section.endBlockOrdinal])).toEqual([[0, 1], [2, 2]]);
    expectSectionsComplete(pageBlocks, pageSections); expectChunksExact(pageBlocks, pageSections, buildKnowledgeChunks(artifactId, pageBlocks));
  });

  it("无结构长文本的父节不超过8192，每原块拥有且只拥有一次primary归属", () => {
    const blocks = Array.from({ length: 60 }, (_, index) => block(index, `第${index}条记录` + "资料".repeat(250)));
    const sections = buildKnowledgeSections(artifactId, blocks);
    expect(sections.length).toBeGreaterThan(1);
    expect(sections.every(section => section.headingPath.length === 0)).toBe(true);
    expectSectionsComplete(blocks, sections); expectChunksExact(blocks, sections, buildKnowledgeChunks(artifactId, blocks));
  });

  it("巨大原块拆连续子节，保同父标题且只保留一个primary归属，引用不重取整个原块", () => {
    const text = "巨型段落𠮷😀。".repeat(5000);
    const blocks = [block(0, text, "markdown", { headingPath: ["报告", "大节"] })];
    const sections = buildKnowledgeSections(artifactId, blocks);
    expect(sections.length).toBeGreaterThan(1);
    expect(sections.map(section => section.headingPath)).toEqual(sections.map(() => ["报告", "大节"]));
    expect(sections.map(section => section.primaryBlockIds)).toEqual([[blocks[0].id], ...sections.slice(1).map(() => [])]);
    expect(sections.every(section => section.startBlockOrdinal === 0 && section.endBlockOrdinal === 0)).toBe(true);
    expect(sections.map(section => section.text).join("")).toBe(text);
    expectSectionsComplete(blocks, sections);
    const chunks = buildKnowledgeChunks(artifactId, blocks);
    expectChunksExact(blocks, sections, chunks);
    expect(chunks.filter(chunk => chunk.sectionId === sections[1].id)[0].spans[0].blockStartOffset)
      .toBe(sections[1].blockSpans![0].blockStartOffset);
    for (const section of sections) expect(section.text.isWellFormed()).toBe(true);
    for (const chunk of chunks) expect(chunk.text.isWellFormed()).toBe(true);
  });

  it.each(["abcdefghij".repeat(1000),
    "The service processes each request in order and records the original result without dropping evidence. ".repeat(100),
    "甲".repeat(3000), "𠮷😀".repeat(1500)])("相邻片段共享约64词元原文，完整片段约512词元且偏移精确", text => {
    const blocks = [block(0, text)], sections = buildKnowledgeSections(artifactId, blocks), chunks = buildKnowledgeChunks(artifactId, blocks);
    expect(sections).toHaveLength(1); expect(chunks.length).toBeGreaterThan(1);
    for (let index = 0; index < chunks.length - 1; index++) {
      expect(chunks[index].tokenCount).toBeGreaterThanOrEqual(511);
      const previous = chunks[index].spans[0], next = chunks[index + 1].spans[0];
      const overlap = text.slice(next.blockStartOffset, previous.blockEndOffset);
      expect(estimateTextTokens(overlap)).toBeGreaterThanOrEqual(63);
      expect(estimateTextTokens(overlap)).toBeLessThanOrEqual(64);
      expect(next.blockStartOffset).toBeGreaterThan(previous.blockStartOffset);
    }
    expectChunksExact(blocks, sections, chunks);
  });

  it("跨原块的片段仅把真实正文区间列为引用，段落分隔符不冒充原文", () => {
    const blocks = Array.from({ length: 20 }, (_, index) => block(index, `原文${index}：` + "内容".repeat(30)));
    const sections = buildKnowledgeSections(artifactId, blocks), chunks = buildKnowledgeChunks(artifactId, blocks);
    expect(chunks.some(chunk => chunk.spans.length > 1)).toBe(true);
    expectChunksExact(blocks, sections, chunks); expectSectionsComplete(blocks, sections);
  });

  it("重复生成和排序输入得到相同身份，v3配置与v2身份互不覆盖", () => {
    const blocks = [block(0, "第一章"), block(1, "同一资料".repeat(700))];
    const chunks = buildKnowledgeChunks(artifactId, blocks);
    expect(buildKnowledgeSections(artifactId, [...blocks].reverse())).toEqual(buildKnowledgeSections(artifactId, blocks));
    expect(buildKnowledgeChunks(artifactId, structuredClone(blocks))).toEqual(chunks);
    const config = resolveKnowledgeChunkerConfig(blocks);
    expect(config.configId).toBe(crypto.createHash("sha256").update("3text2048").digest("hex").slice(0, 16));
    expect(config.configId).not.toBe(legacyKnowledgeChunkerConfigId("text", 2048));
    const legacy = buildLegacyKnowledgeChunks(artifactId, blocks, { targetChars: 2048 });
    expect(chunks.every(chunk => !legacy.some(old => old.id === chunk.id))).toBe(true);
    expect(chunks[0].id).toBe("chunk_" + crypto.createHash("sha256").update(`${config.configId}\0${artifactId}\0${0}`).digest("hex").slice(0, 32));
    expect(knowledgeBlockFingerprint(blocks)).not.toBe(legacyKnowledgeBlockFingerprint(blocks));
  });

  it("旧版必须显式选择，默认1200字符行为和身份仍可准确复现", () => {
    const blocks = [block(0, "文".repeat(3000))], legacy = buildLegacyKnowledgeChunks(artifactId, blocks);
    expect(legacy.map(chunk => chunk.text.length)).toEqual([1200, 1200, 600]);
    expect(buildKnowledgeChunks(artifactId, blocks, { legacyVersion: "2" })).toEqual(legacy);
    expect(resolveLegacyKnowledgeChunkerConfig(blocks).targetChars).toBe(1200);
    expect(knowledgeChunkerConfigId("fixed", 1200, { legacyVersion: "2" })).toBe(legacyKnowledgeChunkerConfigId("fixed", 1200));
    expect(knowledgeBlockFingerprint(blocks, { legacyVersion: "2" })).toBe(legacyKnowledgeBlockFingerprint(blocks));
    expect(legacy.every(chunk => chunk.sectionId === undefined)).toBe(true);
    expect(buildKnowledgeChunks(artifactId, blocks).every(chunk => chunk.sectionId && chunk.tokenCount <= 512)).toBe(true);
  });

  it("空内容块仍有唯一章节归属，不伪造非空片段或引用", () => {
    const blocks = [block(0, ""), block(1, "正文"), block(2, "")];
    const sections = buildKnowledgeSections(artifactId, blocks);
    expectSectionsComplete(blocks, sections); expectChunksExact(blocks, sections, buildKnowledgeChunks(artifactId, blocks));
    expect(buildKnowledgeChunks(artifactId, [])).toEqual([]); expect(buildKnowledgeSections(artifactId, [])).toEqual([]);
  });

  it("不修改输入，重复原块和跨解析产物资料明确拒绝", () => {
    const original = block(0, "不可改写的原文"), blocks = [original], before = structuredClone(blocks);
    buildKnowledgeChunks(artifactId, Object.freeze(blocks)); expect(blocks).toEqual(before);
    expect(() => buildKnowledgeSections(artifactId, [original, original])).toThrow(/unique identities/);
    expect(() => buildKnowledgeSections("another-artifact", blocks)).toThrow(/parse artifact/);
    expect(() => buildKnowledgeChunks(artifactId, blocks, { targetChars: 99 })).toThrow(/targetChars/);
  });
});
