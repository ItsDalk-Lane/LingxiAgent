import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, describe, expect, it } from "vitest";

import {
  KNOWLEDGE_SCHEMA_VERSION,
  KnowledgeStore,
  resolveNotebookConfig,
} from "../lib/knowledge/knowledge-store.ts";

const require = createRequire(import.meta.url);
const Database = require("better-sqlite3");
const tempDirs: string[] = [];

/** V3-V5 研究表全名单：v6 迁移后必须一张不剩。 */
const RESEARCH_TABLES = [
  "research_verification_relations",
  "research_verification_attempts",
  "research_verification_cells",
  "research_verification_steps",
  "research_report_citations",
  "research_reports",
  "research_contradictions",
  "contradiction_checks",
  "contradiction_manifests",
  "claim_packs",
  "claim_evidence",
  "research_claims",
  "research_evidence",
  "evidence_validations",
  "analysis_unit_results",
  "task_attempts",
  "research_jobs",
  "execution_batch_units",
  "execution_batches",
  "analysis_unit_spans",
  "analysis_units",
  "analysis_manifests",
  "research_runs",
  "knowledge_run_citations",
  "knowledge_run_retrievals",
  "knowledge_runs",
  "scope_sources",
  "scope_notebooks",
  "scope_snapshots",
];

function tempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-store-"));
  tempDirs.push(dir);
  return dir;
}

function deterministicIds() {
  let next = 0;
  return (prefix: string) => `${prefix}_${String(++next).padStart(4, "0")}`;
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("KnowledgeStore", () => {
  it("v8 数据迁移：显式 12/500 与 DDL 回填统一清 NULL（跨版本重开验证）", () => {
    const dir = tempDir();
    const dbPath = path.join(dir, "k.db");
    const store = new KnowledgeStore({ dbPath, Database, idGenerator: deterministicIds() });
    const studioId = "studio-a";
    const nb = store.createNotebook({ studioId, name: "甲" });
    store.updateNotebookConfig({ studioId, notebookId: nb.id, retrievalTopK: 12 });
    // 压版本触发 v8 数据迁移。
    store.db.pragma("user_version = 7");
    store.close();
    const upgraded = new KnowledgeStore({ dbPath, Database, idGenerator: deterministicIds() });
    expect(upgraded.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);
    const config = upgraded.getNotebookConfig({ studioId, notebookId: nb.id });
    expect(config.retrievalTopK).toBeNull();
    // 幂等：再次压版本重开仍是 NULL。
    upgraded.db.pragma("user_version = 7");
    upgraded.close();
    const again = new KnowledgeStore({ dbPath, Database, idGenerator: deterministicIds() });
    expect(again.getNotebookConfig({ studioId, notebookId: nb.id }).retrievalTopK).toBeNull();
    again.close();
  });

  it("migrateLegacyGlobalModelRefs：只写未配置列，不覆盖显式配置，幂等", () => {
    const store = new KnowledgeStore({ dbPath: path.join(tempDir(), "k.db"), Database, idGenerator: deterministicIds() });
    const studioId = "studio-a";
    const nbA = store.createNotebook({ studioId, name: "甲" });
    const nbB = store.createNotebook({ studioId, name: "乙" });
    store.updateNotebookConfig({ studioId, notebookId: nbB.id, embeddingModelRef: { id: "own-emb", provider: "volc" } });
    // 软删除笔记本不应被迁移。
    const nbC = store.createNotebook({ studioId, name: "丙" });
    store.db.prepare("UPDATE notebooks SET deleted_at = ? WHERE id = ?").run("2026-01-01T00:00:00.000Z", nbC.id);

    const emb = { id: "global-emb", provider: "volc" };
    const rerank = { id: "global-rerank", provider: "volc" };
    const first = store.migrateLegacyGlobalModelRefs({ embeddingModelRef: emb, rerankModelRef: rerank });
    expect(first.notebooksUpdated).toBe(3); // 甲 embedding+rerank 各 1 行 + 乙 rerank 1 行（软删除丙不计）
    expect(store.getNotebookConfig({ studioId, notebookId: nbA.id })).toEqual({
      embeddingModelRef: emb,
      rerankModelRef: rerank,
      chunkTargetChars: null,
      retrievalTopK: null,
    });
    // 乙的显式嵌入保留，rerank 被补。
    expect(store.getNotebookConfig({ studioId, notebookId: nbB.id })).toMatchObject({
      embeddingModelRef: { id: "own-emb", provider: "volc" },
      rerankModelRef: rerank,
    });
    // 丙（软删除）不写。
    const rowC = store.db.prepare("SELECT embedding_model_ref FROM notebooks WHERE id = ?").get(nbC.id);
    expect(rowC.embedding_model_ref).toBeNull();
    // 幂等：列已写 → 0 行。
    const second = store.migrateLegacyGlobalModelRefs({ embeddingModelRef: emb, rerankModelRef: rerank });
    expect(second.notebooksUpdated).toBe(0);
  });

  it("用独立 user_version 建库，并在重启后保留 Notebook", () => {
    const root = tempDir();
    const dbPath = path.join(root, "knowledge.db");
    const store = new KnowledgeStore({
      dbPath,
      now: () => "2026-08-25T01:00:00.000Z",
      idGenerator: deterministicIds(),
    });

    const notebook = store.createNotebook({ studioId: "studio-a", name: "研究资料" });
    expect(store.db.pragma("journal_mode", { simple: true })).toBe("wal");
    expect(store.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);
    store.close();

    const restarted = new KnowledgeStore({ dbPath });
    expect(restarted.getNotebook({ studioId: "studio-a", notebookId: notebook.id })).toMatchObject({
      id: notebook.id,
      studioId: "studio-a",
      name: "研究资料",
      deletedAt: null,
    });
    expect(restarted.listNotebooks({ studioId: "studio-b" })).toEqual([]);
    restarted.close();
  });

  it("维护 Source 的多 Notebook 成员关系，移除成员不删除历史快照", () => {
    const root = tempDir();
    const store = new KnowledgeStore({
      dbPath: path.join(root, "knowledge.db"),
      now: () => "2026-08-25T02:00:00.000Z",
      idGenerator: deterministicIds(),
    });
    const first = store.createNotebook({ studioId: "studio-a", name: "甲" });
    const second = store.createNotebook({ studioId: "studio-a", name: "乙" });
    const created = store.createSourceWithSnapshot({
      studioId: "studio-a",
      notebookId: first.id,
      sourceType: "file",
      displayName: "事实.txt",
      originMetadata: { fileName: "事实.txt" },
      snapshot: {
        sha256: "a".repeat(64),
        mimeType: "text/plain",
        byteSize: 6,
        storagePath: "sources/src_0003/snap_0004",
      },
    });

    store.addSourceToNotebook({
      studioId: "studio-a",
      notebookId: second.id,
      sourceId: created.source.id,
    });
    expect(store.listNotebookSources({ studioId: "studio-a", notebookId: second.id }))
      .toHaveLength(1);

    store.removeSourceFromNotebook({
      studioId: "studio-a",
      notebookId: first.id,
      sourceId: created.source.id,
    });
    expect(store.listNotebookSources({ studioId: "studio-a", notebookId: first.id })).toEqual([]);
    expect(store.getContentSnapshot({
      studioId: "studio-a",
      snapshotId: created.snapshot.id,
    })).toMatchObject({ sha256: "a".repeat(64), byteSize: 6 });
    store.close();
  });

  it("对跨 Studio 访问、空名称和未来 schema 明确失败", () => {
    const root = tempDir();
    const dbPath = path.join(root, "knowledge.db");
    const store = new KnowledgeStore({ dbPath, idGenerator: deterministicIds() });
    const notebook = store.createNotebook({ studioId: "studio-a", name: "边界" });

    expect(() => store.getNotebook({ studioId: "studio-b", notebookId: notebook.id }))
      .toThrow(/not found/i);
    expect(() => store.createNotebook({ studioId: "studio-a", name: "   " }))
      .toThrow(/name/i);
    store.close();

    const raw = new Database(dbPath);
    raw.pragma(`user_version = ${KNOWLEDGE_SCHEMA_VERSION + 1}`);
    raw.close();
    expect(() => new KnowledgeStore({ dbPath })).toThrow(/newer schema/i);
  });

  it("从 v2 原地升级到当前 schema，并保留已有 Notebook 事实", () => {
    const root = tempDir();
    const dbPath = path.join(root, "knowledge.db");
    const original = new KnowledgeStore({ dbPath, idGenerator: deterministicIds() });
    const notebook = original.createNotebook({ studioId: "studio-a", name: "升级前资料" });
    original.close();

    const raw = new Database(dbPath);
    raw.exec(`
      DROP TABLE IF EXISTS research_verification_relations;
      DROP TABLE IF EXISTS research_verification_attempts;
      DROP TABLE IF EXISTS research_verification_cells;
      DROP TABLE IF EXISTS research_verification_steps;
      DROP TABLE IF EXISTS research_report_citations;
      DROP TABLE IF EXISTS research_reports;
      DROP TABLE IF EXISTS research_contradictions;
      DROP TABLE IF EXISTS contradiction_checks;
      DROP TABLE IF EXISTS contradiction_manifests;
      DROP TABLE IF EXISTS claim_packs;
      DROP TABLE IF EXISTS claim_evidence;
      DROP TABLE IF EXISTS research_claims;
      DROP TABLE IF EXISTS research_evidence;
      DROP TABLE IF EXISTS evidence_validations;
      DROP TABLE IF EXISTS analysis_unit_results;
      DROP TABLE IF EXISTS task_attempts;
      DROP TABLE IF EXISTS research_jobs;
      DROP TABLE IF EXISTS execution_batch_units;
      DROP TABLE IF EXISTS execution_batches;
      DROP TABLE IF EXISTS analysis_unit_spans;
      DROP TABLE IF EXISTS analysis_units;
      DROP TABLE IF EXISTS analysis_manifests;
      DROP TABLE IF EXISTS research_runs;
      DROP TABLE IF EXISTS knowledge_run_citations;
      DROP TABLE IF EXISTS knowledge_run_retrievals;
      DROP TABLE IF EXISTS knowledge_runs;
      DROP TABLE IF EXISTS scope_sources;
      DROP TABLE IF EXISTS scope_notebooks;
      DROP TABLE IF EXISTS scope_snapshots;
      DROP TABLE IF EXISTS ingestion_jobs;
      ALTER TABLE notebooks DROP COLUMN embedding_model_ref;
      ALTER TABLE notebooks DROP COLUMN rerank_model_ref;
      ALTER TABLE notebooks DROP COLUMN chunk_target_chars;
      ALTER TABLE notebooks DROP COLUMN retrieval_top_k;
    `);
    raw.pragma("user_version = 2");
    raw.close();

    const migrated = new KnowledgeStore({ dbPath });
    expect(migrated.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);
    expect(migrated.getNotebook({ studioId: "studio-a", notebookId: notebook.id }).name)
      .toBe("升级前资料");
    // v3-v5 迁移会先重建研究表，v6 在同一升级事务尾段再全部 DROP（派生产物）。
    expect(migrated.db.prepare(
      "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'knowledge_runs'",
    ).get()).toBeUndefined();
    expect(migrated.db.prepare(
      "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'ingestion_jobs'",
    ).get()).toMatchObject({ name: "ingestion_jobs" });
    migrated.close();
  });

  it("从 v5 升级到当前 schema：核心数据保留、研究表消失、v6 新列回填默认值", () => {
    const root = tempDir();
    const dbPath = path.join(root, "knowledge.db");
    const store = new KnowledgeStore({
      dbPath,
      now: () => "2026-08-25T01:00:00.000Z",
      idGenerator: deterministicIds(),
    });
    const notebook = store.createNotebook({ studioId: "studio-a", name: "升级前资料" });
    const imported = store.createSourceWithSnapshot({
      studioId: "studio-a",
      notebookId: notebook.id,
      sourceType: "file",
      displayName: "事实.txt",
      originMetadata: { fileName: "事实.txt" },
      snapshot: {
        sha256: "b".repeat(64),
        mimeType: "text/plain",
        byteSize: 12,
        storagePath: "sources/src_0002/snap_0003",
      },
    });
    const artifact = store.beginParseArtifact({
      studioId: "studio-a",
      contentSnapshotId: imported.snapshot.id,
      parserId: "lingxi-citation",
      parserVersion: "1",
      parserConfigHash: "c".repeat(64),
    });
    store.completeParseArtifact({
      studioId: "studio-a",
      parseArtifactId: artifact.id,
      status: "ready",
      warnings: [],
      semanticArtifactPath: "artifacts/snap_0003/parse_0004.json",
      blocks: [
        { ordinal: 0, text: "第一段", locatorType: "text", locator: { lineStart: 1, lineEnd: 1 } },
        { ordinal: 1, text: "第二段", locatorType: "text", locator: { lineStart: 2, lineEnd: 2 } },
      ],
    });
    store.close();

    // 回滚成 v5 形态：去掉 v6 新增物，补回带数据的研究表（最小 DDL，只验证 DROP 与数据保留）。
    const raw = new Database(dbPath);
    raw.exec(`
      DROP TABLE ingestion_jobs;
      ALTER TABLE notebooks DROP COLUMN embedding_model_ref;
      ALTER TABLE notebooks DROP COLUMN rerank_model_ref;
      ALTER TABLE notebooks DROP COLUMN chunk_target_chars;
      ALTER TABLE notebooks DROP COLUMN retrieval_top_k;
      CREATE TABLE scope_snapshots (id TEXT PRIMARY KEY, studio_id TEXT NOT NULL, created_at TEXT NOT NULL);
      CREATE TABLE knowledge_runs (
        id TEXT PRIMARY KEY, studio_id TEXT NOT NULL, question TEXT NOT NULL, created_at TEXT NOT NULL
      );
      CREATE TABLE research_runs (run_id TEXT PRIMARY KEY, state TEXT NOT NULL, created_at TEXT NOT NULL);
      CREATE TABLE research_reports (run_id TEXT PRIMARY KEY, report_json TEXT NOT NULL, created_at TEXT NOT NULL);
    `);
    raw.prepare("INSERT INTO scope_snapshots (id, studio_id, created_at) VALUES (?, ?, ?)")
      .run("scope_1", "studio-a", "2026-08-20T00:00:00.000Z");
    raw.prepare("INSERT INTO knowledge_runs (id, studio_id, question, created_at) VALUES (?, ?, ?, ?)")
      .run("run_1", "studio-a", "旧提问", "2026-08-20T00:00:00.000Z");
    raw.prepare("INSERT INTO research_runs (run_id, state, created_at) VALUES (?, ?, ?)")
      .run("run_1", "completed", "2026-08-20T00:00:00.000Z");
    raw.prepare("INSERT INTO research_reports (run_id, report_json, created_at) VALUES (?, ?, ?)")
      .run("run_1", "{}", "2026-08-20T00:00:00.000Z");
    raw.pragma("user_version = 5");
    raw.close();

    const migrated = new KnowledgeStore({ dbPath });
    expect(migrated.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);
    // V1-V2 核心事实完整保留。
    expect(migrated.getNotebook({ studioId: "studio-a", notebookId: notebook.id }).name)
      .toBe("升级前资料");
    expect(migrated.listNotebookSources({ studioId: "studio-a", notebookId: notebook.id }))
      .toHaveLength(1);
    expect(migrated.listArtifactBlocks({ studioId: "studio-a", parseArtifactId: artifact.id })
      .map((block) => block.text)).toEqual(["第一段", "第二段"]);
    // 旧行新列：模型引用 NULL；chunk_target_chars 保留 v6 DDL 回填的 1200，
    // retrieval_top_k 被 v8 数据迁移清 NULL（旧默认 12 与手填 12 不可区分，
    // 统一回"无上限"新默认起步）。
    expect(migrated.getNotebookConfig({ studioId: "studio-a", notebookId: notebook.id })).toEqual({
      embeddingModelRef: null,
      rerankModelRef: null,
      chunkTargetChars: 1200,
      retrievalTopK: null,
    });
    // 研究表（含数据）全部消失，ingestion_jobs 就绪。
    const remaining = new Set<string>(migrated.db.prepare(
      "SELECT name FROM sqlite_master WHERE type = 'table'",
    ).all().map((row: any) => row.name));
    for (const table of RESEARCH_TABLES) expect(remaining.has(table)).toBe(false);
    expect(remaining.has("ingestion_jobs")).toBe(true);
    migrated.close();
  });

  it("从 v6 升级到 v7：ingestion_jobs 进度列就位、既有 job 回填默认值", () => {
    const root = tempDir();
    const dbPath = path.join(root, "knowledge.db");
    const store = new KnowledgeStore({
      dbPath,
      now: () => "2026-08-25T01:00:00.000Z",
      idGenerator: deterministicIds(),
    });
    const studioId = "studio-a";
    const notebook = store.createNotebook({ studioId, name: "进度迁移" });
    const imported = store.createSourceWithSnapshot({
      studioId,
      notebookId: notebook.id,
      sourceType: "file",
      displayName: "事实.txt",
      originMetadata: { fileName: "事实.txt" },
      snapshot: {
        sha256: "e".repeat(64),
        mimeType: "text/plain",
        byteSize: 12,
        storagePath: "sources/src_0002/snap_0003",
      },
    });
    const job = store.enqueueIngestionJob({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
      chunkerConfigId: "a".repeat(16),
    });
    store.claimNextIngestionJob();
    store.updateIngestionJobProgress({ studioId, jobId: job.id, done: 3, total: 10 });
    store.close();

    // 回滚成 v6 形态：去掉进度列（v7 之前的库没有这两列）。
    const raw = new Database(dbPath);
    raw.exec(`
      ALTER TABLE ingestion_jobs DROP COLUMN progress_done;
      ALTER TABLE ingestion_jobs DROP COLUMN progress_total;
    `);
    raw.pragma("user_version = 6");
    raw.close();

    const migrated = new KnowledgeStore({ dbPath });
    expect(migrated.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);
    // 既有 job 的状态与归属保留；进度列回填默认值（done 0 / total NULL = 未进入 embed）。
    expect(migrated.getIngestionJob({ studioId, jobId: job.id })).toMatchObject({
      id: job.id,
      status: "running",
      progressDone: 0,
      progressTotal: null,
    });
    migrated.close();
  });

  it("嵌入进度列：仅 running 可写、total 初始化、完成写满与失败/重试重置", () => {
    const root = tempDir();
    let now = "2026-08-25T01:00:00.000Z";
    const store = new KnowledgeStore({
      dbPath: path.join(root, "knowledge.db"),
      now: () => now,
      idGenerator: deterministicIds(),
    });
    const studioId = "studio-a";
    const notebook = store.createNotebook({ studioId, name: "进度" });
    const imported = store.createSourceWithSnapshot({
      studioId,
      notebookId: notebook.id,
      sourceType: "file",
      displayName: "事实.txt",
      originMetadata: { fileName: "事实.txt" },
      snapshot: {
        sha256: "f".repeat(64),
        mimeType: "text/plain",
        byteSize: 12,
        storagePath: "sources/src_0002/snap_0003",
      },
    });
    const job = store.enqueueIngestionJob({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
      chunkerConfigId: "a".repeat(16),
    });
    // 新库直建 v7：进度列默认 done=0 / total=NULL。
    expect(store.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);
    expect(job).toMatchObject({ progressDone: 0, progressTotal: null });

    // 仅 running 可写：queued 态直接拒绝。
    expect(() => store.updateIngestionJobProgress({ studioId, jobId: job.id, done: 1, total: 4 }))
      .toThrow(/not running/i);

    expect(store.claimNextIngestionJob()?.id).toBe(job.id);
    // total 首次给出时初始化；后续省略 total 保留已知值（失败批间不丢分母）。
    expect(store.updateIngestionJobProgress({ studioId, jobId: job.id, done: 1, total: 4 }))
      .toMatchObject({ progressDone: 1, progressTotal: 4 });
    expect(store.updateIngestionJobProgress({ studioId, jobId: job.id, done: 3 }))
      .toMatchObject({ progressDone: 3, progressTotal: 4 });
    // 边界校验：done 不能超过 total，均为非负整数。
    expect(() => store.updateIngestionJobProgress({ studioId, jobId: job.id, done: 5, total: 4 }))
      .toThrow(/exceed total/i);
    expect(() => store.updateIngestionJobProgress({ studioId, jobId: job.id, done: -1 }))
      .toThrow(/non-negative/i);

    // 完成：total 已知时写满（UI 端 100% 落定）。
    expect(store.completeIngestionJob({ studioId, jobId: job.id }))
      .toMatchObject({ status: "done", progressDone: 4, progressTotal: 4 });

    // 失败（带退避回 queued）与手动重试都重置进度：幂等重跑防 UI 显示旧进度回退。
    const rebuild = store.enqueueIngestionJob({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
      chunkerConfigId: "b".repeat(16),
    });
    store.claimNextIngestionJob();
    store.updateIngestionJobProgress({ studioId, jobId: rebuild.id, done: 2, total: 6 });
    expect(store.failIngestionJob({
      studioId,
      jobId: rebuild.id,
      error: "embed_timeout",
      retryAfter: "2026-08-25T01:00:30.000Z",
    })).toMatchObject({ status: "queued", progressDone: 0, progressTotal: null });
    now = "2026-08-25T01:01:00.000Z";
    store.claimNextIngestionJob();
    store.updateIngestionJobProgress({ studioId, jobId: rebuild.id, done: 2, total: 6 });
    store.failIngestionJob({ studioId, jobId: rebuild.id, error: "embed_timeout" });
    expect(store.requeueIngestionJob({ studioId, jobId: rebuild.id }))
      .toMatchObject({ status: "queued", progressDone: 0, progressTotal: null });
    store.close();
  });

  it("Notebook 配置：默认值、部分更新、NULL 继承与解析链", () => {
    const root = tempDir();
    const store = new KnowledgeStore({
      dbPath: path.join(root, "knowledge.db"),
      now: () => "2026-08-25T01:00:00.000Z",
      idGenerator: deterministicIds(),
    });
    const studioId = "studio-a";
    const notebook = store.createNotebook({ studioId, name: "配置" });

    // 新 Notebook 默认（v8）：模型引用 NULL、数值 NULL（自动分块/无上限召回）。
    const defaults = store.getNotebookConfig({ studioId, notebookId: notebook.id });
    expect(defaults).toEqual({
      embeddingModelRef: null,
      rerankModelRef: null,
      chunkTargetChars: null,
      retrievalTopK: null,
    });
    // 解析（v8 起仅笔记本列，无全局偏好级）：NULL 原样透出，语义在消费侧解释。
    expect(resolveNotebookConfig(defaults)).toEqual({
      embeddingModelRef: null,
      rerankModelRef: null,
      chunkTargetChars: null,
      retrievalTopK: null,
    });

    // 部分更新：只动给定字段。
    const updated = store.updateNotebookConfig({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: { id: "emb", provider: "volc" },
      chunkTargetChars: 800,
      retrievalTopK: 20,
    });
    expect(updated).toEqual({
      embeddingModelRef: { id: "emb", provider: "volc" },
      rerankModelRef: null,
      chunkTargetChars: 800,
      retrievalTopK: 20,
    });
    // v8：解析仅读笔记本列。
    expect(resolveNotebookConfig(updated)).toEqual({
      embeddingModelRef: { id: "emb", provider: "volc" },
      rerankModelRef: null,
      chunkTargetChars: 800,
      retrievalTopK: 20,
    });

    // null 清除 → 回到继承/内置默认；omitted 字段不变。
    const cleared = store.updateNotebookConfig({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: null,
      chunkTargetChars: null,
    });
    expect(cleared).toEqual({
      embeddingModelRef: null,
      rerankModelRef: null,
      chunkTargetChars: null,
      retrievalTopK: 20,
    });
    expect(resolveNotebookConfig(cleared)).toEqual({
      embeddingModelRef: null,
      rerankModelRef: null,
      chunkTargetChars: null,
      retrievalTopK: 20,
    });

    // 校验与边界。
    expect(() => store.updateNotebookConfig({ studioId, notebookId: notebook.id, chunkTargetChars: 50 }))
      .toThrow(/chunkTargetChars/);
    expect(() => store.updateNotebookConfig({ studioId, notebookId: notebook.id, retrievalTopK: 0 }))
      .toThrow(/retrievalTopK/);
    expect(() => store.updateNotebookConfig({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: { id: "emb" },
    })).toThrow(/provider/);
    expect(() => store.updateNotebookConfig({ studioId, notebookId: notebook.id }))
      .toThrow(/at least one field/);
    expect(() => store.getNotebookConfig({ studioId: "studio-b", notebookId: notebook.id }))
      .toThrow(/not found/i);
    store.close();
  });

  it("ingestion job 全生命周期：入队去重、认领、phase 推进、退避重试与计数", () => {
    const root = tempDir();
    let now = "2026-08-25T01:00:00.000Z";
    const store = new KnowledgeStore({
      dbPath: path.join(root, "knowledge.db"),
      now: () => now,
      idGenerator: deterministicIds(),
    });
    const studioId = "studio-a";
    const notebook = store.createNotebook({ studioId, name: "摄入" });
    const imported = store.createSourceWithSnapshot({
      studioId,
      notebookId: notebook.id,
      sourceType: "file",
      displayName: "事实.txt",
      originMetadata: { fileName: "事实.txt" },
      snapshot: {
        sha256: "b".repeat(64),
        mimeType: "text/plain",
        byteSize: 12,
        storagePath: "sources/src_0002/snap_0003",
      },
    });

    // 入队；同 notebook+source 的活跃 job 去重返回。
    const job = store.enqueueIngestionJob({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
      chunkerConfigId: "a".repeat(16),
    });
    expect(job).toMatchObject({
      notebookId: notebook.id,
      sourceId: imported.source.id,
      artifactId: null,
      phase: "parse",
      status: "queued",
      attempt: 0,
      retryAfter: null,
      error: null,
      chunkerConfigId: "a".repeat(16),
    });
    expect(store.enqueueIngestionJob({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
      chunkerConfigId: "b".repeat(16),
    }).id).toBe(job.id);
    expect(store.getLatestIngestionJobForSource({ studioId, sourceId: imported.source.id })?.id)
      .toBe(job.id);
    expect(() => store.enqueueIngestionJob({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
      chunkerConfigId: "xyz",
    })).toThrow(/chunkerConfigId/);
    expect(() => store.getIngestionJob({ studioId: "studio-b", jobId: job.id }))
      .toThrow(/not found/i);

    // 认领：queued → running，队列随后为空。
    expect(store.claimNextIngestionJob()).toMatchObject({ id: job.id, status: "running" });
    expect(store.claimNextIngestionJob()).toBeNull();

    // phase 推进并绑定 parse artifact；非法 phase 与 done 被拒。
    const artifact = store.beginParseArtifact({
      studioId,
      contentSnapshotId: imported.snapshot.id,
      parserId: "lingxi-citation",
      parserVersion: "1",
      parserConfigHash: "c".repeat(64),
    });
    expect(store.updateIngestionJobPhase({
      studioId,
      jobId: job.id,
      phase: "chunk",
      artifactId: artifact.id,
    })).toMatchObject({ phase: "chunk", artifactId: artifact.id });
    expect(() => store.updateIngestionJobPhase({ studioId, jobId: job.id, phase: "done" }))
      .toThrow(/phase/i);
    expect(() => store.updateIngestionJobPhase({ studioId, jobId: job.id, phase: "sideways" }))
      .toThrow(/phase/i);

    // 未配置嵌入模型 → pending_embedding 显式终态，不再被认领。
    store.updateIngestionJobPhase({ studioId, jobId: job.id, phase: "fts_index" });
    store.updateIngestionJobPhase({ studioId, jobId: job.id, phase: "embed" });
    expect(store.markIngestionJobPendingEmbedding({ studioId, jobId: job.id }))
      .toMatchObject({ phase: "embed", status: "pending_embedding" });
    expect(store.claimNextIngestionJob()).toBeNull();

    // 模型就绪信号 → 批量置回 queued。
    expect(store.requeuePendingEmbeddingIngestionJobs()).toBe(1);
    expect(store.getIngestionJob({ studioId, jobId: job.id }).status).toBe("queued");

    // 失败带退避：attempt + 1 回 queued，未到 retry_after 不可认领。
    expect(store.claimNextIngestionJob()?.status).toBe("running");
    expect(store.failIngestionJob({
      studioId,
      jobId: job.id,
      error: "embed_timeout",
      retryAfter: "2026-08-25T01:00:30.000Z",
    })).toMatchObject({
      status: "queued",
      attempt: 1,
      error: "embed_timeout",
      retryAfter: "2026-08-25T01:00:30.000Z",
    });
    expect(store.claimNextIngestionJob()).toBeNull();

    // 退避到期可再认领；不带 retryAfter 的失败 → failed 终态。
    now = "2026-08-25T01:01:00.000Z";
    expect(store.claimNextIngestionJob()?.id).toBe(job.id);
    expect(store.failIngestionJob({ studioId, jobId: job.id, error: "embed_timeout" }))
      .toMatchObject({ status: "failed", attempt: 2, error: "embed_timeout" });

    // 手动重试：attempt 归零、错误清除，phase 保留在失败点续跑。
    expect(store.requeueIngestionJob({ studioId, jobId: job.id })).toMatchObject({
      status: "queued",
      attempt: 0,
      error: null,
      retryAfter: null,
      phase: "embed",
    });
    expect(store.claimNextIngestionJob()?.id).toBe(job.id);
    expect(store.completeIngestionJob({ studioId, jobId: job.id }))
      .toMatchObject({ phase: "done", status: "done", error: null });
    expect(store.countIngestionJobsByStatus({ studioId })).toEqual({
      queued: 0,
      running: 0,
      pending_embedding: 0,
      failed: 0,
      done: 1,
    });

    // done 不挡重建：同 source 可再次入队；列表与计数按过滤条件返回。
    const rebuild = store.enqueueIngestionJob({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
      chunkerConfigId: "b".repeat(16),
    });
    expect(rebuild.id).not.toBe(job.id);
    expect(store.listIngestionJobs({ studioId, sourceId: imported.source.id })).toHaveLength(2);
    expect(store.listIngestionJobs({ studioId, statuses: ["queued"] })).toHaveLength(1);
    expect(store.listIngestionJobs({ studioId, notebookId: notebook.id, statuses: ["done"] }))
      .toHaveLength(1);
    expect(store.countIngestionJobsByStatus({ studioId, notebookId: notebook.id }))
      .toEqual({ queued: 1, running: 0, pending_embedding: 0, failed: 0, done: 1 });

    // 状态机守门：非 running 不能推进/完成，非 failed 不能手动重试。
    expect(() => store.completeIngestionJob({ studioId, jobId: rebuild.id })).toThrow(/not running/i);
    expect(() => store.requeueIngestionJob({ studioId, jobId: rebuild.id })).toThrow(/failed/i);
    store.close();
  });
});
