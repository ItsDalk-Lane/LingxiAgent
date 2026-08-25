import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, describe, expect, it } from "vitest";

import {
  KNOWLEDGE_SCHEMA_VERSION,
  KnowledgeStore,
} from "../lib/knowledge/knowledge-store.ts";

const require = createRequire(import.meta.url);
const Database = require("better-sqlite3");
const tempDirs: string[] = [];

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
      DROP TABLE research_verification_relations;
      DROP TABLE research_verification_attempts;
      DROP TABLE research_verification_cells;
      DROP TABLE research_verification_steps;
      DROP TABLE research_report_citations;
      DROP TABLE research_reports;
      DROP TABLE research_contradictions;
      DROP TABLE contradiction_checks;
      DROP TABLE contradiction_manifests;
      DROP TABLE claim_packs;
      DROP TABLE claim_evidence;
      DROP TABLE research_claims;
      DROP TABLE research_evidence;
      DROP TABLE evidence_validations;
      DROP TABLE analysis_unit_results;
      DROP TABLE task_attempts;
      DROP TABLE research_jobs;
      DROP TABLE execution_batch_units;
      DROP TABLE execution_batches;
      DROP TABLE analysis_unit_spans;
      DROP TABLE analysis_units;
      DROP TABLE analysis_manifests;
      DROP TABLE research_runs;
      DROP TABLE knowledge_run_citations;
      DROP TABLE knowledge_run_retrievals;
      DROP TABLE knowledge_runs;
      DROP TABLE scope_sources;
      DROP TABLE scope_notebooks;
      DROP TABLE scope_snapshots;
    `);
    raw.pragma("user_version = 2");
    raw.close();

    const migrated = new KnowledgeStore({ dbPath });
    expect(migrated.db.pragma("user_version", { simple: true })).toBe(KNOWLEDGE_SCHEMA_VERSION);
    expect(migrated.getNotebook({ studioId: "studio-a", notebookId: notebook.id }).name)
      .toBe("升级前资料");
    expect(migrated.db.prepare(
      "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'knowledge_runs'",
    ).get()).toMatchObject({ name: "knowledge_runs" });
    migrated.close();
  });
});
