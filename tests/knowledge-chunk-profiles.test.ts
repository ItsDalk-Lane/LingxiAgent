import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, describe, expect, it } from "vitest";

import { KNOWLEDGE_CHUNK_TARGET_CHARS, computeAutoChunkTargetChars, knowledgeChunkerConfigId, legacyKnowledgeChunkerConfigId } from "../lib/knowledge/chunker.ts";
import {
  KNOWLEDGE_SCHEMA_VERSION,
  KnowledgeStore,
  knowledgeRetrievalProfileKey,
} from "../lib/knowledge/knowledge-store.ts";

const require = createRequire(import.meta.url);
const Database = require("better-sqlite3");
const tempDirs: string[] = [];

function tempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-chunk-profiles-"));
  tempDirs.push(dir);
  return dir;
}

function deterministicIds() {
  let next = 0;
  return (prefix: string) => `${prefix}_${String(++next).padStart(4, "0")}`;
}

function openStore(dbPath: string, options?: { getEmbeddingModelContextWindow?: (ref: any) => number | null }) {
  return new KnowledgeStore({
    dbPath,
    Database,
    idGenerator: deterministicIds(),
    now: () => "2026-08-29T00:00:00.000Z",
    ...options,
  });
}

/** 把 v9 库回滚成 v8 形态（去掉 v9 新增物），模拟旧库升级。 */
function rollbackToV8(dbPath: string) {
  const raw = new Database(dbPath);
  raw.exec(`
    DROP TABLE retrieval_profiles;
    DROP TABLE chunk_profiles;
    ALTER TABLE notebooks DROP COLUMN retrieval_profile_id;
  `);
  raw.pragma("user_version = 8");
  raw.close();
}

let seedCounter = 0;
function seedNotebookWithSource(store: KnowledgeStore, studioId: string, name: string, notebook?: { id: string }) {
  const nb = notebook ?? store.createNotebook({ studioId, name });
  seedCounter += 1;
  const imported = store.createSourceWithSnapshot({
    studioId,
    notebookId: nb.id,
    sourceType: "pasted_text",
    displayName: `${name}-${seedCounter}.txt`,
    originMetadata: {},
    snapshot: {
      sha256: "a".repeat(64),
      mimeType: "text/plain",
      byteSize: 10,
      storagePath: `sources/${nb.id}/snap-${seedCounter}`,
    },
  });
  return { notebook: nb, sourceId: imported.source.id };
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("KnowledgeStore v9：ChunkProfile / RetrievalProfile", () => {
  it("v9 schema：两张身份表与 notebooks.retrieval_profile_id 列就位", () => {
    const store = openStore(path.join(tempDir(), "knowledge.db"));
    expect(store.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);
    expect(KNOWLEDGE_SCHEMA_VERSION).toBe(19);

    // v12（Phase 5 生命周期治理）：orphan 标记列、取消留痕列与活跃 job 部分唯一索引。
    const sourceColumns = new Set<string>(
      store.db.pragma("table_info(sources)").map((column: any) => column.name),
    );
    expect(sourceColumns.has("orphaned_at")).toBe(true);
    const jobColumns = new Set<string>(
      store.db.pragma("table_info(ingestion_jobs)").map((column: any) => column.name),
    );
    expect(jobColumns.has("cancelled_at")).toBe(true);
    const indexNames = new Set<string>(
      store.db.prepare("SELECT name FROM sqlite_master WHERE type = 'index'").all()
        .map((row: any) => row.name),
    );
    expect(indexNames.has("idx_ingestion_jobs_active")).toBe(true);
    expect(indexNames.has("idx_sources_orphaned")).toBe(true);

    const tables = new Set<string>(store.db.prepare(
      "SELECT name FROM sqlite_master WHERE type = 'table'",
    ).all().map((row: any) => row.name));
    expect(tables.has("chunk_profiles")).toBe(true);
    expect(tables.has("retrieval_profiles")).toBe(true);
    // v11（Phase 4 KnowledgeTurnScope）：两张轮级权限天花板表就位。
    expect(tables.has("knowledge_turn_scopes")).toBe(true);
    expect(tables.has("knowledge_turn_scope_sources")).toBe(true);
    // v13（Phase 7 覆盖规划）：结构化覆盖计划表就位（禁 CoT 落库）。
    expect(tables.has("knowledge_coverage_plans")).toBe(true);
    // v14（Phase 9 EXHAUSTIVE 覆盖执行）：run/shard 执行事实表就位。
    expect(tables.has("coverage_runs")).toBe(true);
    expect(tables.has("coverage_shards")).toBe(true);
    const scopeColumns = new Set<string>(
      store.db.pragma("table_info(knowledge_turn_scopes)").map((column: any) => column.name),
    );
    for (const column of [
      "id", "turn_id", "session_path", "studio_id", "notebook_ids_json", "status", "created_at",
    ]) {
      expect(scopeColumns.has(column)).toBe(true);
    }
    const scopeSourceColumns = new Set<string>(
      store.db.pragma("table_info(knowledge_turn_scope_sources)").map((column: any) => column.name),
    );
    for (const column of [
      "scope_id", "source_id", "content_snapshot_id", "parse_artifact_id", "notebook_ids_json",
    ]) {
      expect(scopeSourceColumns.has(column)).toBe(true);
    }

    const chunkColumns = new Set<string>(
      store.db.pragma("table_info(chunk_profiles)").map((column: any) => column.name),
    );
    for (const column of [
      "id", "profile_hash", "strategy", "target_chars", "target_chars_source",
      "chunker_version", "structural_options_json", "profile_type", "created_at",
    ]) {
      expect(chunkColumns.has(column)).toBe(true);
    }
    const retrievalColumns = new Set<string>(
      store.db.pragma("table_info(retrieval_profiles)").map((column: any) => column.name),
    );
    for (const column of [
      "id", "profile_key", "chunk_profile_id", "embedding_model_ref",
      "rerank_model_ref", "retrieval_top_k", "created_at",
    ]) {
      expect(retrievalColumns.has(column)).toBe(true);
    }
    const notebookColumns = new Set<string>(
      store.db.pragma("table_info(notebooks)").map((column: any) => column.name),
    );
    expect(notebookColumns.has("retrieval_profile_id")).toBe(true);
    store.close();
  });

  it("findOrCreateChunkProfile：同配置同身份（幂等一行），不同配置不同 profile", () => {
    const store = openStore(path.join(tempDir(), "knowledge.db"));
    const first = store.findOrCreateChunkProfile({
      strategy: "markdown",
      targetChars: 1500,
      targetCharsSource: "explicit",
    });
    expect(first.id).toBe(`cp_${knowledgeChunkerConfigId("markdown", 1500)}`);
    expect(first.profileHash).toBe(knowledgeChunkerConfigId("markdown", 1500));
    expect(first.profileType).toBe("standard");
    expect(first.chunkerVersion).toBe("3");
    expect(first.targetCharsSource).toBe("explicit");

    const again = store.findOrCreateChunkProfile({
      strategy: "markdown",
      targetChars: 1500,
      targetCharsSource: "explicit",
    });
    expect(again.id).toBe(first.id);
    expect(store.db.prepare("SELECT COUNT(*) AS n FROM chunk_profiles").get().n).toBe(1);

    const otherSize = store.findOrCreateChunkProfile({
      strategy: "markdown",
      targetChars: 2400,
      targetCharsSource: "explicit",
    });
    const otherStrategy = store.findOrCreateChunkProfile({
      strategy: "pdf",
      targetChars: 1500,
      targetCharsSource: "explicit",
    });
    expect(otherSize.id).not.toBe(first.id);
    expect(otherStrategy.id).not.toBe(first.id);
    expect(store.db.prepare("SELECT COUNT(*) AS n FROM chunk_profiles").get().n).toBe(3);

    // 非法输入显式拒绝。
    expect(() => store.findOrCreateChunkProfile({
      strategy: "markdown",
      targetChars: 10,
      targetCharsSource: "explicit",
    })).toThrowError(/targetChars/);
    expect(() => store.findOrCreateChunkProfile({
      strategy: "weird",
      targetChars: 1500,
      targetCharsSource: "explicit",
    })).toThrowError(/strategy/);
    store.close();
  });

  it("findOrCreateRetrievalProfile：同配置同 key 幂等；引用未知 chunk profile 显式报错", () => {
    const store = openStore(path.join(tempDir(), "knowledge.db"));
    const chunkProfile = store.findOrCreateChunkProfile({
      strategy: "text",
      targetChars: 1200,
      targetCharsSource: "explicit",
    });
    const embedding = { id: "emb-1", provider: "volc" };
    const rerank = { id: "rr-1", provider: "volc" };
    const first = store.findOrCreateRetrievalProfile({
      chunkProfileId: chunkProfile.id,
      embeddingModelRef: embedding,
      rerankModelRef: rerank,
      retrievalTopK: 20,
    });
    expect(first.id).toBe(`rp_${knowledgeRetrievalProfileKey({
      chunkProfileHash: chunkProfile.profileHash,
      embeddingModelRef: embedding,
      rerankModelRef: rerank,
      retrievalTopK: 20,
    })}`);
    expect(first.embeddingModelRef).toEqual(embedding);
    expect(first.retrievalTopK).toBe(20);

    const again = store.findOrCreateRetrievalProfile({
      chunkProfileId: chunkProfile.id,
      embeddingModelRef: embedding,
      rerankModelRef: rerank,
      retrievalTopK: 20,
    });
    expect(again.id).toBe(first.id);
    expect(store.db.prepare("SELECT COUNT(*) AS n FROM retrieval_profiles").get().n).toBe(1);

    const otherTopK = store.findOrCreateRetrievalProfile({
      chunkProfileId: chunkProfile.id,
      embeddingModelRef: embedding,
      rerankModelRef: rerank,
      retrievalTopK: null,
    });
    expect(otherTopK.id).not.toBe(first.id);

    expect(() => store.findOrCreateRetrievalProfile({
      chunkProfileId: "cp_missing",
      embeddingModelRef: embedding,
    })).toThrowError(/Chunk profile not found/);
    store.close();
  });

  it("resolveNotebookRetrievalProfile：生效配置 → find-or-create → 绑定；重复调用幂等", () => {
    const store = openStore(path.join(tempDir(), "knowledge.db"));
    const studioId = "studio-a";
    const nb = store.createNotebook({ studioId, name: "显式" });
    store.updateNotebookConfig({ studioId, notebookId: nb.id, chunkTargetChars: 1500, retrievalTopK: 30 });

    const resolved = store.resolveNotebookRetrievalProfile({
      studioId,
      notebookId: nb.id,
      strategy: "markdown",
    });
    expect(resolved.bindingUpdated).toBe(true);
    expect(resolved.chunkProfile.targetChars).toBe(1500);
    expect(resolved.chunkProfile.targetCharsSource).toBe("explicit");
    expect(resolved.retrievalProfile.retrievalTopK).toBe(30);
    const boundRow = store.db.prepare(
      "SELECT retrieval_profile_id FROM notebooks WHERE id = ?",
    ).get(nb.id);
    expect(boundRow.retrieval_profile_id).toBe(resolved.retrievalProfile.id);

    const second = store.resolveNotebookRetrievalProfile({
      studioId,
      notebookId: nb.id,
      strategy: "markdown",
    });
    expect(second.bindingUpdated).toBe(false);
    expect(second.retrievalProfile.id).toBe(resolved.retrievalProfile.id);

    // 不同策略 → 不同 chunk profile → 不同 retrieval profile，绑定原子切换，旧行保留。
    const switched = store.resolveNotebookRetrievalProfile({
      studioId,
      notebookId: nb.id,
      strategy: "pdf",
    });
    expect(switched.bindingUpdated).toBe(true);
    expect(switched.retrievalProfile.id).not.toBe(resolved.retrievalProfile.id);
    expect(store.getRetrievalProfile({ profileId: resolved.retrievalProfile.id }).id)
      .toBe(resolved.retrievalProfile.id);
    store.close();
  });

  it("resolveNotebookRetrievalProfile：新默认分块使用固定粒度并记 auto，模型窗口不改变配置", () => {
    const store = openStore(path.join(tempDir(), "knowledge.db"));
    const studioId = "studio-a";
    const nb = store.createNotebook({ studioId, name: "自动" });
    store.updateNotebookConfig({
      studioId,
      notebookId: nb.id,
      embeddingModelRef: { id: "emb-1", provider: "volc" },
    });

    const resolved = store.resolveNotebookRetrievalProfile({
      studioId,
      notebookId: nb.id,
      strategy: "text",
      getEmbeddingModelContextWindow: () => 32768,
    });
    expect(resolved.chunkProfile.targetChars).toBe(KNOWLEDGE_CHUNK_TARGET_CHARS);
    expect(resolved.chunkProfile.targetCharsSource).toBe("auto");

    // 窗口未知时仍保持同一固定默认，与摄入和查询的身份一致。
    const fallback = store.resolveNotebookRetrievalProfile({
      studioId,
      notebookId: nb.id,
      strategy: "fixed",
      getEmbeddingModelContextWindow: () => null,
    });
    expect(fallback.chunkProfile.targetChars).toBe(KNOWLEDGE_CHUNK_TARGET_CHARS);
    expect(fallback.chunkProfile.targetCharsSource).toBe("auto");
    store.close();
  });

  it("同配置笔记本共享同一 RetrievalProfile 身份", () => {
    const store = openStore(path.join(tempDir(), "knowledge.db"));
    const studioId = "studio-a";
    const nbA = store.createNotebook({ studioId, name: "甲" });
    const nbB = store.createNotebook({ studioId, name: "乙" });
    for (const nb of [nbA, nbB]) {
      store.updateNotebookConfig({ studioId, notebookId: nb.id, chunkTargetChars: 1500 });
    }
    const a = store.resolveNotebookRetrievalProfile({ studioId, notebookId: nbA.id, strategy: "markdown" });
    const b = store.resolveNotebookRetrievalProfile({ studioId, notebookId: nbB.id, strategy: "markdown" });
    expect(a.retrievalProfile.id).toBe(b.retrievalProfile.id);
    expect(a.chunkProfile.id).toBe(b.chunkProfile.id);
    store.close();
  });

  it("updateNotebookConfig：配置变更同步刷新绑定，旧 profile 行保留（additive）", () => {
    const store = openStore(path.join(tempDir(), "knowledge.db"));
    const studioId = "studio-a";
    const nb = store.createNotebook({ studioId, name: "切换" });

    // 从未绑定：配置更新不伪造策略，绑定保持 NULL（留给首次 resolve 惰性建绑）。
    store.updateNotebookConfig({ studioId, notebookId: nb.id, chunkTargetChars: 1500 });
    expect(store.db.prepare(
      "SELECT retrieval_profile_id FROM notebooks WHERE id = ?",
    ).get(nb.id).retrieval_profile_id).toBeNull();

    const before = store.resolveNotebookRetrievalProfile({ studioId, notebookId: nb.id, strategy: "markdown" });
    store.updateNotebookConfig({ studioId, notebookId: nb.id, chunkTargetChars: 2400 });
    const boundRow = store.db.prepare(
      "SELECT retrieval_profile_id FROM notebooks WHERE id = ?",
    ).get(nb.id);
    expect(boundRow.retrieval_profile_id).not.toBe(before.retrievalProfile.id);
    const after = store.getRetrievalProfile({ profileId: boundRow.retrieval_profile_id });
    const afterChunk = store.getChunkProfile({ profileId: after.chunkProfileId });
    expect(afterChunk.strategy).toBe("markdown"); // 策略继承自原绑定（配置变更不改策略）
    expect(afterChunk.targetChars).toBe(2400);
    expect(afterChunk.targetCharsSource).toBe("explicit");
    // 旧 profile 未被删除（additive；零引用 GC 属后续波次）。
    expect(store.getRetrievalProfile({ profileId: before.retrievalProfile.id }).id)
      .toBe(before.retrievalProfile.id);
    store.close();
  });

  it("v8→v9 迁移回填：可推导指纹写真实值（standard），不可推导标 legacy 不伪造", () => {
    const dbPath = path.join(tempDir(), "knowledge.db");
    const store = openStore(dbPath);
    const studioId = "studio-a";

    // 显式配置笔记本 + 一条与当前配置匹配的摄入历史。
    const explicit = seedNotebookWithSource(store, studioId, "explicit");
    store.updateNotebookConfig({ studioId, notebookId: explicit.notebook.id, chunkTargetChars: 1500 });
    const matchedHash = legacyKnowledgeChunkerConfigId("markdown", 1500);
    store.enqueueIngestionJob({
      studioId,
      notebookId: explicit.notebook.id,
      sourceId: explicit.sourceId,
      chunkerConfigId: matchedHash,
    });

    // 自动配置笔记本（无嵌入引用 → 内置兜底窗口）+ 匹配的摄入历史。
    const auto = seedNotebookWithSource(store, studioId, "auto");
    const autoHash = legacyKnowledgeChunkerConfigId("fixed", computeAutoChunkTargetChars(null));
    store.enqueueIngestionJob({
      studioId,
      notebookId: auto.notebook.id,
      sourceId: auto.sourceId,
      chunkerConfigId: autoHash,
    });

    // 无法推导的历史指纹（ notebook 配置后来改过，旧值无从得知）：挂在第二个源的 job 上。
    const legacySource = seedNotebookWithSource(store, studioId, "auto", auto.notebook);
    store.enqueueIngestionJob({
      studioId,
      notebookId: auto.notebook.id,
      sourceId: legacySource.sourceId,
      chunkerConfigId: autoHash,
    });
    const legacyHash = "1".repeat(16);
    store.db.prepare("UPDATE ingestion_jobs SET chunker_config_id = ? WHERE source_id = ?")
      .run(legacyHash, legacySource.sourceId);
    store.close();

    rollbackToV8(dbPath);
    const migrated = openStore(dbPath);
    expect(migrated.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);

    const matched = migrated.getChunkProfile({ profileHash: matchedHash });
    expect(matched.profileType).toBe("standard");
    expect(matched.strategy).toBe("markdown");
    expect(matched.targetChars).toBe(1500);
    expect(matched.targetCharsSource).toBe("explicit");
    expect(matched.chunkerVersion).toBe("2");

    const autoProfile = migrated.getChunkProfile({ profileHash: autoHash });
    expect(autoProfile.profileType).toBe("standard");
    expect(autoProfile.targetChars).toBe(computeAutoChunkTargetChars(null));
    expect(autoProfile.targetCharsSource).toBe("auto");

    const legacy = migrated.getChunkProfile({ profileHash: legacyHash });
    expect(legacy.profileType).toBe("legacy");
    expect(legacy.strategy).toBeNull();
    expect(legacy.targetChars).toBeNull();
    expect(legacy.chunkerVersion).toBeNull();
    expect(legacy.id).toBe(`cp_${legacyHash}`);

    // 绑定列迁移后保持 NULL（惰性建绑），不伪造检索配置。
    const bindings = migrated.db.prepare(
      "SELECT retrieval_profile_id FROM notebooks",
    ).all();
    for (const row of bindings) expect(row.retrieval_profile_id).toBeNull();

    // 幂等：再次压版本重放迁移不报错、不产生重复行。
    const profileCount = migrated.db.prepare("SELECT COUNT(*) AS n FROM chunk_profiles").get().n;
    migrated.close();
    rollbackToV8(dbPath);
    const replayed = openStore(dbPath);
    expect(replayed.db.prepare("SELECT COUNT(*) AS n FROM chunk_profiles").get().n).toBe(profileCount);
    replayed.close();
  });

  it("getChunkProfile / getRetrievalProfile：缺失显式 KNOWLEDGE_NOT_FOUND", () => {
    const store = openStore(path.join(tempDir(), "knowledge.db"));
    expect(() => store.getChunkProfile({ profileId: "cp_missing" }))
      .toThrowError(/Chunk profile not found/);
    expect(() => store.getChunkProfile({ profileHash: knowledgeChunkerConfigId("fixed", 1200) }))
      .toThrowError(/Chunk profile not found/);
    expect(() => store.getRetrievalProfile({ profileId: "rp_missing" }))
      .toThrowError(/Retrieval profile not found/);
    expect(() => store.getChunkProfile({})).toThrowError(/requires profileId or profileHash/);
    store.close();
  });
});
