import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import {
  buildKnowledgeChunks,
  knowledgeBlockFingerprint,
  resolveKnowledgeChunkerConfig,
} from "../lib/knowledge/chunker.ts";
import { KnowledgeIndexStore } from "../lib/knowledge/knowledge-index-store.ts";
import type { KnowledgeBlock } from "../lib/knowledge/types.ts";
import {
  buildFtsLiteralQuery,
  normalizeSearchText,
  tokenizeSearchText,
} from "../lib/search/search-text.ts";

const tempDirs: string[] = [];
const stores: KnowledgeIndexStore[] = [];

function tempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-search-"));
  tempDirs.push(dir);
  return dir;
}

function block(parseArtifactId: string, id: string, ordinal: number, text: string): KnowledgeBlock {
  return {
    id,
    parseArtifactId,
    ordinal,
    text,
    textSha256: crypto.createHash("sha256").update(text, "utf8").digest("hex"),
    locatorType: "text",
    locator: { lineStart: ordinal + 1, lineEnd: ordinal + 1 },
  };
}

function openIndex(dbPath: string) {
  const store = new KnowledgeIndexStore({ dbPath });
  stores.push(store);
  return store;
}

function indexBlocks(store: KnowledgeIndexStore, parseArtifactId: string, blocks: KnowledgeBlock[]) {
  store.replaceArtifactChunks({
    parseArtifactId,
    blockFingerprint: knowledgeBlockFingerprint(blocks),
    chunkProfileHash: resolveKnowledgeChunkerConfig(blocks).configId,
    chunks: buildKnowledgeChunks(parseArtifactId, blocks),
  });
}

function scopeOf(parseArtifactId: string, blocks: KnowledgeBlock[]) {
  return {
    parseArtifactId,
    chunkProfileHash: resolveKnowledgeChunkerConfig(blocks).configId,
  };
}

afterEach(() => {
  for (const store of stores.splice(0)) store.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe("Knowledge search primitives", () => {
  it("统一正规化中文与英文，并只生成转义后的 FTS 字面量", () => {
    expect(normalizeSearchText("  ＡＰＩ_V2   交付日期  ")).toBe("api_v2 交付日期");
    const tokens = tokenizeSearchText("API_V2 交付日期");
    expect(tokens).toEqual(expect.arrayContaining(["api_v2", "交付", "日期", "交付日", "付日期"]));
    expect(buildFtsLiteralQuery('交付日期 " OR *')).toMatch(/^"[^"]+"(?: OR "[^"]+")*$/u);
  });

  it("按稳定边界切块，并让每个字符区间精确指回原始 Block", () => {
    const parseArtifactId = "parse-a";
    const text = `${"甲".repeat(1199)}😀${"乙".repeat(320)}`;
    const blocks = [block(parseArtifactId, "block-a", 0, text)];
    const first = buildKnowledgeChunks(parseArtifactId, blocks);
    const second = buildKnowledgeChunks(parseArtifactId, blocks);

    expect(first.length).toBeGreaterThan(1);
    expect(first.map(chunk => chunk.id)).toEqual(second.map(chunk => chunk.id));
    expect(first.map(chunk => chunk.text).join("")).toBe(text);
    expect(first.every(chunk => !/[\uD800-\uDBFF]$/u.test(chunk.text))).toBe(true);
    expect(first.flatMap(chunk => chunk.spans).map(span => [
      span.blockStartOffset,
      span.blockEndOffset,
    ])).toEqual([
      [0, 1199],
      [1199, text.length],
    ]);
  });

  it("搜索结果严格受冻结产物集合约束", () => {
    const store = openIndex(path.join(tempDir(), "knowledge-fts.db"));
    const blocksA = [block("parse-a", "block-a", 0, "苹果项目的交付日期是九月十五日。")];
    const blocksB = [block("parse-b", "block-b", 0, "火星项目的预算是八百万元。")];
    indexBlocks(store, "parse-a", blocksA);
    indexBlocks(store, "parse-b", blocksB);

    const onlyA = store.search({ scopes: [scopeOf("parse-a", blocksA)], query: "项目", limit: 12 });
    expect(onlyA.map(result => result.parseArtifactId)).toEqual(["parse-a"]);
    expect(onlyA[0].text).toContain("苹果项目");

    const both = store.search({ scopes: [scopeOf("parse-a", blocksA), scopeOf("parse-b", blocksB)], query: "项目", limit: 12 });
    expect(new Set(both.map(result => result.parseArtifactId))).toEqual(new Set(["parse-a", "parse-b"]));
  });

  it("索引文件损坏时只丢弃缓存并建立空白健康索引", () => {
    const dbPath = path.join(tempDir(), "knowledge-fts.db");
    const store = openIndex(dbPath);
    const blocks = [block("parse-a", "block-a", 0, "可恢复的搜索内容")];
    indexBlocks(store, "parse-a", blocks);
    const variantId = store.resolveChunkIndexVariant("parse-a", scopeOf("parse-a", blocks).chunkProfileHash)!.id;
    expect(store.listVariantChunks(variantId)).toHaveLength(1);
    store.close();

    fs.writeFileSync(dbPath, "not-a-sqlite-database", "utf8");
    const recovered = openIndex(dbPath);
    expect(recovered.health()).toEqual({ status: "ready" });
    expect(recovered.resolveChunkIndexVariant("parse-a", scopeOf("parse-a", blocks).chunkProfileHash)).toBeNull();
    expect(recovered.listVariantChunks(variantId)).toEqual([]);
  });
});
