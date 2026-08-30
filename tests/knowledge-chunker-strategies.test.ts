import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import {
  buildKnowledgeChunks,
  knowledgeBlockFingerprint,
  knowledgeChunkerConfigId,
  resolveKnowledgeChunkerConfig,
  type KnowledgeChunkDraft,
} from "../lib/knowledge/chunker.ts";
import { KnowledgeIndexStore } from "../lib/knowledge/knowledge-index-store.ts";
import type { KnowledgeBlock } from "../lib/knowledge/types.ts";

const tempDirs: string[] = [];
const stores: KnowledgeIndexStore[] = [];

function tempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-chunker-"));
  tempDirs.push(dir);
  return dir;
}

function openIndex(dbPath: string) {
  const store = new KnowledgeIndexStore({ dbPath });
  stores.push(store);
  return store;
}

afterEach(() => {
  for (const store of stores.splice(0)) store.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function makeBlock(
  parseArtifactId: string,
  ordinal: number,
  text: string,
  locatorType: KnowledgeBlock["locatorType"],
  locator: Record<string, unknown>,
): KnowledgeBlock {
  return {
    id: `${parseArtifactId}-block-${ordinal}`,
    parseArtifactId,
    ordinal,
    text,
    textSha256: crypto.createHash("sha256").update(text, "utf8").digest("hex"),
    locatorType,
    locator,
  };
}

const mdBlock = (ordinal: number, text: string, headingPath: string[], line: number) =>
  makeBlock("parse-md", ordinal, text, "markdown", { headingPath, lineStart: line, lineEnd: line });

const textBlock = (ordinal: number, text: string, line: number) =>
  makeBlock("parse-text", ordinal, text, "text", { lineStart: line, lineEnd: line });

const pdfBlock = (ordinal: number, text: string, page: number) =>
  makeBlock("parse-pdf", ordinal, text, "pdf", { page, pageCharStart: 0, pageCharEnd: text.length });

const htmlBlock = (ordinal: number, text: string, headingPath: string[]) =>
  makeBlock("parse-html", ordinal, text, "html", {
    structuralPath: `html > body > p:nth-of-type(${ordinal + 1})`,
    headingPath,
  });

/** span 完整性：每个 span 区间在 chunk 文本与 block 文本中切片必须一致；返回覆盖清单。 */
function expectSpanIntegrity(blocks: KnowledgeBlock[], chunks: KnowledgeChunkDraft[]) {
  const byId = new Map(blocks.map(block => [block.id, block]));
  const coverage: string[] = [];
  for (const chunk of chunks) {
    expect(chunk.spans.length).toBeGreaterThan(0);
    for (const span of chunk.spans) {
      const block = byId.get(span.blockId);
      expect(block).toBeDefined();
      expect(chunk.text.slice(span.chunkStartOffset, span.chunkEndOffset))
        .toBe(block!.text.slice(span.blockStartOffset, span.blockEndOffset));
      coverage.push(`${span.blockId}:${span.blockStartOffset}-${span.blockEndOffset}`);
    }
  }
  return coverage;
}

/** 每个 block 的全文恰好按序覆盖一次（未被切开的场景）。 */
function expectFullCoverage(blocks: KnowledgeBlock[], chunks: KnowledgeChunkDraft[]) {
  expect(expectSpanIntegrity(blocks, chunks))
    .toEqual(blocks.map(block => `${block.id}:0-${block.text.length}`));
}

describe("markdown 策略", () => {
  it("按 headingPath 首元素切节并注入面包屑，确定性可复现", () => {
    const blocks = [
      mdBlock(0, "没有标题的引言", [], 1),
      mdBlock(1, "产品概述", ["产品概述"], 3),
      mdBlock(2, "这是概述正文", ["产品概述"], 4),
      mdBlock(3, "安装指南", ["安装指南"], 6),
      mdBlock(4, "先安装依赖", ["安装指南"], 7),
    ];
    expect(resolveKnowledgeChunkerConfig(blocks).strategy).toBe("markdown");
    const chunks = buildKnowledgeChunks("parse-md", blocks);

    expect(chunks).toHaveLength(3);
    // 首个标题前的序言节无面包屑。
    expect(chunks[0].text).toBe("没有标题的引言");
    // 面包屑注入 chunk 头部，标题词进入检索文本。
    expect(chunks[1].text.startsWith("产品概述\n\n")).toBe(true);
    expect(chunks[1].text).toContain("这是概述正文");
    expect(chunks[2].text.startsWith("安装指南\n\n")).toBe(true);
    expectFullCoverage(blocks, chunks);

    const second = buildKnowledgeChunks("parse-md", blocks);
    expect(second.map(chunk => chunk.id)).toEqual(chunks.map(chunk => chunk.id));
  });

  it("超长节在空行段落边界二分，后续 chunk 携带子标题面包屑", () => {
    const filler = () => "文".repeat(300);
    const blocks = [
      mdBlock(0, "指南", ["指南"], 1),
      mdBlock(1, filler(), ["指南", "背景"], 3),
      mdBlock(2, filler(), ["指南", "背景"], 4),
      mdBlock(3, filler(), ["指南", "背景"], 6),
      mdBlock(4, filler(), ["指南", "背景"], 7),
      mdBlock(5, filler(), ["指南", "参数"], 9),
      mdBlock(6, filler(), ["指南", "参数"], 10),
      mdBlock(7, filler(), ["指南", "参数"], 12),
      mdBlock(8, filler(), ["指南", "参数"], 13),
    ];
    const chunks = buildKnowledgeChunks("parse-md", blocks);

    // 总量 2418 > 1200*1.5，二分在两段背景与两段参数之间。
    expect(chunks).toHaveLength(2);
    expect(chunks[0].text.startsWith("指南\n\n")).toBe(true);
    expect(chunks[1].text.startsWith("指南 > 参数\n\n")).toBe(true);
    expect(chunks[0].text.length - "指南\n\n".length).toBeLessThanOrEqual(1800);
    expect(chunks[1].text.length - "指南 > 参数\n\n".length).toBeLessThanOrEqual(1800);
    // 段落不被切开：每个 block 完整出现一次。
    expectFullCoverage(blocks, chunks);
  });
});

describe("text 策略（章节启发式）", () => {
  it("按“第X章/回”聚合章节边界", () => {
    const blocks = [
      textBlock(0, "引子：没有章节标题之前的内容", 1),
      textBlock(1, "第一章 风起", 3),
      textBlock(2, "正文第一段", 4),
      textBlock(3, "正文第二段", 6),
      textBlock(4, "第108章 云涌", 8),
      textBlock(5, "云涌正文", 9),
      textBlock(6, "第十二回 落幕", 11),
      textBlock(7, "落幕正文", 12),
    ];
    expect(resolveKnowledgeChunkerConfig(blocks).strategy).toBe("text");
    const chunks = buildKnowledgeChunks("parse-text", blocks);

    expect(chunks).toHaveLength(4);
    // 无面包屑注入，章节标题行本身就是 chunk 开头。
    expect(chunks[0].text).toBe("引子：没有章节标题之前的内容");
    expect(chunks[1].text.startsWith("第一章 风起")).toBe(true);
    expect(chunks[1].text).toContain("正文第二段");
    expect(chunks[2].text.startsWith("第108章 云涌")).toBe(true);
    expect(chunks[3].text.startsWith("第十二回 落幕")).toBe(true);
    expectFullCoverage(blocks, chunks);
  });

  it("识别英文 Chapter N 与序章/楔子等特殊章节名", () => {
    const english = [
      textBlock(0, "Chapter 1", 1),
      textBlock(1, "It was a bright day.", 2),
      textBlock(2, "Chapter 2", 4),
      textBlock(3, "Things changed.", 5),
    ];
    const englishChunks = buildKnowledgeChunks("parse-text", english);
    expect(resolveKnowledgeChunkerConfig(english).strategy).toBe("text");
    expect(englishChunks).toHaveLength(2);
    expect(englishChunks[0].text.startsWith("Chapter 1")).toBe(true);
    expect(englishChunks[1].text.startsWith("Chapter 2")).toBe(true);
    expectFullCoverage(english, englishChunks);

    const special = [
      textBlock(0, "序章", 1),
      textBlock(1, "这是序章的内容", 2),
      textBlock(2, "楔子", 4),
      textBlock(3, "这是楔子的内容", 5),
      textBlock(4, "尾声", 7),
      textBlock(5, "这是尾声的内容", 8),
      textBlock(6, "终章", 10),
      textBlock(7, "这是终章的内容", 11),
      textBlock(8, "番外", 13),
      textBlock(9, "这是番外的内容", 14),
      textBlock(10, "后记", 16),
      textBlock(11, "这是后记的内容", 17),
    ];
    const specialChunks = buildKnowledgeChunks("parse-text", special);
    expect(specialChunks.map(chunk => chunk.text.split("\n")[0]))
      .toEqual(["序章", "楔子", "尾声", "终章", "番外", "后记"]);
    expectFullCoverage(special, specialChunks);
  });

  it("前 200 个 Block 无章节结构时回退固定大小策略", () => {
    const blocks = Array.from({ length: 250 }, (_, index) =>
      textBlock(index, `普通第${index}行内容`, index + 1));
    expect(resolveKnowledgeChunkerConfig(blocks).strategy).toBe("fixed");

    const chunks = buildKnowledgeChunks("parse-text", blocks);
    expect(chunks.length).toBeGreaterThan(1);
    expect(chunks.every(chunk => chunk.text.length <= 1200)).toBe(true);
    expect(chunks[0].text.startsWith("普通第0行内容")).toBe(true);
    expectSpanIntegrity(blocks, chunks);
  });

  it("章节探测只看前 200 个 Block", () => {
    const make = (headingIndex: number) => Array.from({ length: 260 }, (_, index) =>
      textBlock(index, index === headingIndex ? "第一章 风起" : `普通第${index}行`, index + 1));
    expect(resolveKnowledgeChunkerConfig(make(199)).strategy).toBe("text");
    expect(resolveKnowledgeChunkerConfig(make(200)).strategy).toBe("fixed");
  });
});

describe("pdf 策略", () => {
  it("按页分组且页内容不跨页合并", () => {
    const blocks = [
      pdfBlock(0, "第一页第一行", 1),
      pdfBlock(1, "第一页第二行", 1),
      pdfBlock(2, "第二页内容", 2),
      pdfBlock(3, "第三页内容", 3),
    ];
    expect(resolveKnowledgeChunkerConfig(blocks).strategy).toBe("pdf");
    const chunks = buildKnowledgeChunks("parse-pdf", blocks);

    expect(chunks).toHaveLength(3);
    expect(chunks[0].text).toBe("第一页第一行\n\n第一页第二行");
    expect(chunks[1].text).toBe("第二页内容");
    expect(chunks[2].text).toBe("第三页内容");
    expectFullCoverage(blocks, chunks);
  });

  it("页内超长在行块边界二分", () => {
    const blocks = [0, 1, 2, 3].map(index => pdfBlock(index, "数".repeat(700), 1));
    const chunks = buildKnowledgeChunks("parse-pdf", blocks);

    expect(chunks).toHaveLength(2);
    expect(chunks.every(chunk => chunk.text.length <= 1800)).toBe(true);
    expect(chunks[0].text).toBe(`${"数".repeat(700)}\n\n${"数".repeat(700)}`);
    expectFullCoverage(blocks, chunks);
  });

  it("单块超软上限时页内回退固定大小边界切分", () => {
    const big = "载".repeat(3000);
    const chunks = buildKnowledgeChunks("parse-pdf", [pdfBlock(0, big, 1)]);

    expect(chunks).toHaveLength(3);
    expect(chunks.map(chunk => chunk.text)).toEqual([
      big.slice(0, 1200),
      big.slice(1200, 2400),
      big.slice(2400),
    ]);
    expect(chunks.flatMap(chunk => chunk.spans).map(span => [span.blockStartOffset, span.blockEndOffset]))
      .toEqual([[0, 1200], [1200, 2400], [2400, 3000]]);
  });
});

describe("html 策略", () => {
  it("按 headingPath 切节并注入面包屑", () => {
    const blocks = [
      htmlBlock(0, "介绍", ["介绍"]),
      htmlBlock(1, "介绍正文段落", ["介绍"]),
      htmlBlock(2, "用法", ["用法"]),
      htmlBlock(3, "用法正文段落", ["用法"]),
    ];
    expect(resolveKnowledgeChunkerConfig(blocks).strategy).toBe("html");
    const chunks = buildKnowledgeChunks("parse-html", blocks);

    expect(chunks).toHaveLength(2);
    expect(chunks[0].text.startsWith("介绍\n\n")).toBe(true);
    expect(chunks[1].text.startsWith("用法\n\n")).toBe(true);
    expectFullCoverage(blocks, chunks);
  });
});

describe("固定大小回退", () => {
  it("无匹配 locatorType 时回退固定大小策略", () => {
    const blocks = [{ ...textBlock(0, "未知类型内容", 1), locatorType: "unknown" as any }];
    expect(resolveKnowledgeChunkerConfig(blocks).strategy).toBe("fixed");
    const chunks = buildKnowledgeChunks("parse-text", blocks);
    expect(chunks).toHaveLength(1);
    expect(chunks[0].text).toBe("未知类型内容");
  });

  it("空 Block 列表不产生 chunk", () => {
    expect(resolveKnowledgeChunkerConfig([]).strategy).toBe("fixed");
    expect(buildKnowledgeChunks("parse-empty", [])).toEqual([]);
  });

  it("非法 targetChars 直接抛错", () => {
    const blocks = [textBlock(0, "内容", 1)];
    for (const bad of [0, -1, 99, 1.5, Number.NaN]) {
      expect(() => buildKnowledgeChunks("parse-text", blocks, { targetChars: bad })).toThrow(/targetChars/u);
    }
  });
});

describe("chunkerConfigId 缓存键", () => {
  it("格式为 16 位 hex，由版本前缀 + 策略 + targetChars 决定", () => {
    const expected = crypto.createHash("sha256").update("2markdown1200", "utf8").digest("hex").slice(0, 16);
    expect(knowledgeChunkerConfigId("markdown", 1200)).toBe(expected);
    expect(knowledgeChunkerConfigId("markdown", 1200)).toMatch(/^[0-9a-f]{16}$/u);
  });

  it("targetChars 或策略变化 → 不同 configId → 不同 chunk id", () => {
    const blocks = [mdBlock(0, "标题", ["标题"], 1), mdBlock(1, "正文内容", ["标题"], 2)];
    const base = resolveKnowledgeChunkerConfig(blocks);
    expect(base.configId).not.toBe(resolveKnowledgeChunkerConfig(blocks, { targetChars: 800 }).configId);
    const novel = [textBlock(0, "第一章 风起", 1), textBlock(1, "正文", 2)];
    expect(base.configId).not.toBe(resolveKnowledgeChunkerConfig(novel).configId);

    const defaultIds = buildKnowledgeChunks("parse-md", blocks).map(chunk => chunk.id);
    const smallerIds = buildKnowledgeChunks("parse-md", blocks, { targetChars: 800 }).map(chunk => chunk.id);
    expect(smallerIds).not.toEqual(defaultIds);

    // chunk id 编入 configId。
    const expectedId = `chunk_${crypto.createHash("sha256")
      .update(`${base.configId}\0${"parse-md"}\0${0}`, "utf8")
      .digest("hex")
      .slice(0, 32)}`;
    expect(buildKnowledgeChunks("parse-md", blocks)[0].id).toBe(expectedId);
  });

  it("targetChars 参数实际控制分块大小", () => {
    const lines = Array.from({ length: 200 }, (_, index) =>
      textBlock(index, `无结构文本第${index}行内容填充`, index + 1));
    const small = buildKnowledgeChunks("parse-text", lines, { targetChars: 400 });
    const large = buildKnowledgeChunks("parse-text", lines, { targetChars: 1200 });
    expect(small.length).toBeGreaterThan(large.length);
    expect(small.every(chunk => chunk.text.length <= 400)).toBe(true);
  });

  it("KnowledgeIndexStore 以 configId 判断缓存：配置变化即失效并整体重建", () => {
    const store = openIndex(path.join(tempDir(), "knowledge-fts.db"));
    const blocks = [mdBlock(0, "标题", ["标题"], 1), mdBlock(1, "需要检索的正文内容", ["标题"], 2)];
    const fingerprint = knowledgeBlockFingerprint(blocks);
    const config1200 = resolveKnowledgeChunkerConfig(blocks).configId;
    const config800 = resolveKnowledgeChunkerConfig(blocks, { targetChars: 800 }).configId;

    store.replaceArtifactChunks({
      parseArtifactId: "parse-md",
      blockFingerprint: fingerprint,
      chunkerVersion: config1200,
      chunks: buildKnowledgeChunks("parse-md", blocks),
    });
    expect(store.hasArtifactFingerprint("parse-md", fingerprint, config1200)).toBe(true);
    // 仅 targetChars 变化即判定缓存失效。
    expect(store.hasArtifactFingerprint("parse-md", fingerprint, config800)).toBe(false);

    store.replaceArtifactChunks({
      parseArtifactId: "parse-md",
      blockFingerprint: fingerprint,
      chunkerVersion: config800,
      chunks: buildKnowledgeChunks("parse-md", blocks, { targetChars: 800 }),
    });
    expect(store.hasArtifactFingerprint("parse-md", fingerprint, config800)).toBe(true);
    expect(store.hasArtifactFingerprint("parse-md", fingerprint, config1200)).toBe(false);
  });
});
