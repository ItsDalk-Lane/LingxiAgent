import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, describe, expect, it } from "vitest";

import {
  buildLegacyKnowledgeChunks as buildKnowledgeChunks,
  legacyKnowledgeBlockFingerprint as knowledgeBlockFingerprint,
  legacyKnowledgeChunkerConfigId as knowledgeChunkerConfigId,
} from "../lib/knowledge/chunker.ts";
import {
  KnowledgeIndexStore,
  knowledgeChunkIndexVariantId,
} from "../lib/knowledge/knowledge-index-store.ts";
import type { KnowledgeBlock } from "../lib/knowledge/types.ts";
import { buildSearchDocumentText } from "../lib/search/search-text.ts";

const require = createRequire(import.meta.url);

const tempDirs: string[] = [];
const stores: KnowledgeIndexStore[] = [];

function tempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-variants-"));
  tempDirs.push(dir);
  return dir;
}

function openIndex(dbPath: string) {
  const store = new KnowledgeIndexStore({ dbPath });
  stores.push(store);
  return store;
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

afterEach(() => {
  for (const store of stores.splice(0)) store.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

/** 与 v1 createSchema 完全一致的旧库 DDL，用于构造迁移前的知识-fts.db。 */
const V1_DDL = `
  CREATE TABLE artifact_indexes (
    parse_artifact_id TEXT PRIMARY KEY,
    block_fingerprint TEXT NOT NULL,
    chunker_version TEXT NOT NULL,
    indexed_at TEXT NOT NULL
  );

  CREATE TABLE knowledge_chunks (
    row_id INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    parse_artifact_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
    text TEXT NOT NULL,
    token_count INTEGER NOT NULL CHECK(token_count > 0),
    search_text TEXT NOT NULL,
    spans_json TEXT NOT NULL,
    UNIQUE(parse_artifact_id, ordinal)
  );

  CREATE INDEX idx_knowledge_chunks_artifact
    ON knowledge_chunks(parse_artifact_id, ordinal);

  CREATE VIRTUAL TABLE knowledge_chunks_fts USING fts5(
    text,
    search_text,
    content=knowledge_chunks,
    content_rowid=row_id,
    tokenize='unicode61'
  );

  CREATE TRIGGER knowledge_chunks_ai AFTER INSERT ON knowledge_chunks BEGIN
    INSERT INTO knowledge_chunks_fts(rowid, text, search_text)
    VALUES (new.row_id, new.text, new.search_text);
  END;
  CREATE TRIGGER knowledge_chunks_ad AFTER DELETE ON knowledge_chunks BEGIN
    INSERT INTO knowledge_chunks_fts(knowledge_chunks_fts, rowid, text, search_text)
    VALUES ('delete', old.row_id, old.text, old.search_text);
  END;
  CREATE TRIGGER knowledge_chunks_au AFTER UPDATE ON knowledge_chunks BEGIN
    INSERT INTO knowledge_chunks_fts(knowledge_chunks_fts, rowid, text, search_text)
    VALUES ('delete', old.row_id, old.text, old.search_text);
    INSERT INTO knowledge_chunks_fts(rowid, text, search_text)
    VALUES (new.row_id, new.text, new.search_text);
  END;
`;

function createV1Database(dbPath: string) {
  const mod = require("better-sqlite3");
  const Database = mod?.default || mod;
  const db = new Database(dbPath);
  try {
    db.exec(V1_DDL);
    const insertIndex = db.prepare(`
      INSERT INTO artifact_indexes (parse_artifact_id, block_fingerprint, chunker_version, indexed_at)
      VALUES (?, ?, ?, ?)
    `);
    const insertChunk = db.prepare(`
      INSERT INTO knowledge_chunks (id, parse_artifact_id, ordinal, text, token_count, search_text, spans_json)
      VALUES (?, ?, ?, ?, ?, ?, ?)
    `);
    const span = {
      blockId: "block-x",
      blockStartOffset: 0,
      blockEndOffset: 10,
      chunkStartOffset: 0,
      chunkEndOffset: 10,
    };
    // 正常行：chunker_version 已是 chunkerConfigId（16 hex）。
    insertIndex.run("parse-ok", "fp-ok", knowledgeChunkerConfigId("fixed", 1200), "2026-08-01T00:00:00.000Z");
    insertChunk.run(
      "chunk_ok0",
      "parse-ok",
      0,
      "迁移民工项目的交付日期是九月十五日。",
      10,
      buildSearchDocumentText("迁移民工项目的交付日期是九月十五日。"),
      JSON.stringify([span]),
    );
    // 远古行：chunker_version 仍是常量版本号，无法推导真实 profile，迁移必须跳过。
    insertIndex.run("parse-legacy", "fp-legacy", "1", "2026-01-01T00:00:00.000Z");
    insertChunk.run(
      "chunk_legacy0",
      "parse-legacy",
      0,
      "远古常量版本号索引内容。",
      6,
      buildSearchDocumentText("远古常量版本号索引内容。"),
      JSON.stringify([span]),
    );
    db.pragma("user_version = 1");
  } finally {
    db.close();
  }
}

describe("ChunkIndexVariant 身份", () => {
  it("变体 id 由 (parseArtifactId, chunkProfileHash) 确定性生成", () => {
    const profileA = knowledgeChunkerConfigId("fixed", 1200);
    const profileB = knowledgeChunkerConfigId("fixed", 800);

    const id = knowledgeChunkIndexVariantId("parse-a", profileA);
    expect(id).toMatch(/^civ_[0-9a-f]{32}$/u);
    expect(knowledgeChunkIndexVariantId("parse-a", profileA)).toBe(id);
    expect(knowledgeChunkIndexVariantId("parse-a", profileB)).not.toBe(id);
    expect(knowledgeChunkIndexVariantId("parse-b", profileA)).not.toBe(id);
  });

  it("同一 artifact 的多个分块配置变体并存且检索互不串扰", () => {
    const store = openIndex(path.join(tempDir(), "knowledge-fts.db"));
    const profileA = knowledgeChunkerConfigId("fixed", 1200);
    const profileB = knowledgeChunkerConfigId("fixed", 800);
    const blocks = [block("parse-a", "block-a", 0, "苹果项目的交付日期是九月十五日。")];
    const fingerprint = knowledgeBlockFingerprint(blocks);

    store.replaceArtifactChunks({
      parseArtifactId: "parse-a",
      chunkProfileHash: profileA,
      blockFingerprint: fingerprint,
      chunks: buildKnowledgeChunks("parse-a", blocks, { targetChars: 1200 }),
    });
    store.replaceArtifactChunks({
      parseArtifactId: "parse-a",
      chunkProfileHash: profileB,
      blockFingerprint: fingerprint,
      chunks: buildKnowledgeChunks("parse-a", blocks, { targetChars: 800 }),
    });

    const variantA = store.resolveChunkIndexVariant("parse-a", profileA)!;
    const variantB = store.resolveChunkIndexVariant("parse-a", profileB)!;
    expect(variantA.status).toBe("ready");
    expect(variantB.status).toBe("ready");
    expect(store.listVariantChunks(variantA.id)).toHaveLength(1);
    expect(store.listVariantChunks(variantB.id)).toHaveLength(1);
    expect(store.listVariantChunks(variantA.id)[0].id)
      .not.toBe(store.listVariantChunks(variantB.id)[0].id);

    expect(store.hasArtifactFingerprint("parse-a", profileA, fingerprint)).toBe(true);
    expect(store.hasArtifactFingerprint("parse-a", profileB, fingerprint)).toBe(true);
    expect(store.hasArtifactFingerprint("parse-a", profileA, "fp-other")).toBe(false);

    const scopedA = store.search({
      scopes: [{ parseArtifactId: "parse-a", chunkProfileHash: profileA }],
      query: "项目",
      limit: 12,
    });
    expect(scopedA.map(result => result.chunkIndexVariantId)).toEqual([variantA.id]);

    // 重建变体 A 不影响变体 B 的命中。
    store.replaceArtifactChunks({
      parseArtifactId: "parse-a",
      chunkProfileHash: profileA,
      blockFingerprint: "fp-new",
      chunks: buildKnowledgeChunks("parse-a", blocks, { targetChars: 1200 }),
    });
    expect(store.hasArtifactFingerprint("parse-a", profileA, fingerprint)).toBe(false);
    expect(store.hasArtifactFingerprint("parse-a", profileB, fingerprint)).toBe(true);
    expect(store.listVariantChunks(variantB.id)).toHaveLength(1);
  });

  it("ensure/setStatus 驱动 building → ready 生命周期，未 ready 变体不参与检索", () => {
    const store = openIndex(path.join(tempDir(), "knowledge-fts.db"));
    const profile = knowledgeChunkerConfigId("fixed", 1200);
    const blocks = [block("parse-a", "block-a", 0, "苹果项目的交付日期是九月十五日。")];
    const fingerprint = knowledgeBlockFingerprint(blocks);

    const building = store.ensureChunkIndexVariant({
      parseArtifactId: "parse-a",
      chunkProfileHash: profile,
      blockFingerprint: fingerprint,
    });
    expect(building.status).toBe("building");
    // 幂等：重复 ensure 不回退状态、不新建行。
    expect(store.ensureChunkIndexVariant({
      parseArtifactId: "parse-a",
      chunkProfileHash: profile,
      blockFingerprint: fingerprint,
    }).id).toBe(building.id);

    expect(store.hasArtifactFingerprint("parse-a", profile, fingerprint)).toBe(false);
    expect(store.search({
      scopes: [{ parseArtifactId: "parse-a", chunkProfileHash: profile }],
      query: "项目",
      limit: 12,
    })).toEqual([]);

    store.replaceArtifactChunks({
      parseArtifactId: "parse-a",
      chunkProfileHash: profile,
      blockFingerprint: fingerprint,
      chunks: buildKnowledgeChunks("parse-a", blocks),
    });
    expect(store.resolveChunkIndexVariant("parse-a", profile)!.status).toBe("ready");
    expect(store.hasArtifactFingerprint("parse-a", profile, fingerprint)).toBe(true);

    const retiring = store.setChunkIndexVariantStatus(building.id, "retiring");
    expect(retiring.status).toBe("retiring");
    expect(store.hasArtifactFingerprint("parse-a", profile, fingerprint)).toBe(false);
    expect(() => store.setChunkIndexVariantStatus("civ_missing", "ready")).toThrowError(/unknown/iu);
  });
});

describe("knowledge-fts.db v1 → v2 迁移", () => {
  it("回填变体并搬移 chunk，不丢索引数据，artifact_indexes 退役", () => {
    const dbPath = path.join(tempDir(), "knowledge-fts.db");
    createV1Database(dbPath);

    const store = openIndex(dbPath);
    expect(store.db.pragma("user_version", { simple: true })).toBe(4);
    expect(store.health()).toEqual({ status: "ready" });

    const profile = knowledgeChunkerConfigId("fixed", 1200);
    const variant = store.resolveChunkIndexVariant("parse-ok", profile)!;
    expect(variant.id).toBe(knowledgeChunkIndexVariantId("parse-ok", profile));
    expect(variant.status).toBe("ready");
    expect(variant.blockFingerprint).toBe("fp-ok");
    expect(variant.createdAt).toBe("2026-08-01T00:00:00.000Z");
    expect(store.hasArtifactFingerprint("parse-ok", profile, "fp-ok")).toBe(true);

    const chunks = store.listVariantChunks(variant.id);
    expect(chunks).toHaveLength(1);
    expect(chunks[0].id).toBe("chunk_ok0");
    expect(chunks[0].chunkIndexVariantId).toBe(variant.id);
    expect(chunks[0].text).toContain("迁移民工项目");
    expect(chunks[0].spans[0].blockId).toBe("block-x");

    // FTS 随迁移 rebuild，检索立即可用。
    const hits = store.search({
      scopes: [{ parseArtifactId: "parse-ok", chunkProfileHash: profile }],
      query: "交付日期",
      limit: 12,
    });
    expect(hits.map(hit => hit.id)).toEqual(["chunk_ok0"]);

    // 远古常量版本号行不伪造 profile：变体与 chunk 均不搬移，等待按新身份重建。
    expect(store.resolveChunkIndexVariant("parse-legacy", profile)).toBeNull();
    expect(store.search({
      scopes: [{ parseArtifactId: "parse-legacy", chunkProfileHash: profile }],
      query: "远古",
      limit: 12,
    })).toEqual([]);

    // artifact_indexes 已退役，库内不再有第二处身份真相。
    const legacyTable = store.db.prepare(`
      SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'artifact_indexes'
    `).get();
    expect(legacyTable).toBeUndefined();
  });

  it("迁移后的库再次打开走 v2 快路径，数据保持稳定", () => {
    const dbPath = path.join(tempDir(), "knowledge-fts.db");
    createV1Database(dbPath);
    openIndex(dbPath).close();

    const reopened = openIndex(dbPath);
    const profile = knowledgeChunkerConfigId("fixed", 1200);
    const variant = reopened.resolveChunkIndexVariant("parse-ok", profile)!;
    expect(reopened.listVariantChunks(variant.id)).toHaveLength(1);
    expect(reopened.search({
      scopes: [{ parseArtifactId: "parse-ok", chunkProfileHash: profile }],
      query: "交付日期",
      limit: 12,
    })).toHaveLength(1);
  });
});
