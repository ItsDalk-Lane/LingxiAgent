import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, describe, expect, it } from "vitest";
import { KnowledgeStore } from "../lib/knowledge/knowledge-store.ts";
import { ResearchStore } from "../lib/knowledge/research/research-store.ts";

interface SqliteDatabase {
  prepare<T = Record<string, unknown>>(sql: string): { all(): T[] };
  exec(sql: string): SqliteDatabase;
  pragma(sql: string, options?: { simple: boolean }): unknown;
  close(): void;
  readonly inTransaction: boolean;
}
type SchemaEntry = { type: string; name: string; tbl_name: string; sql: string | null };
type ColumnInfo = { name: string; type: string; notnull: number; dflt_value: string | null; pk: number };
const Database = createRequire(import.meta.url)("better-sqlite3") as new (filename: string) => SqliteDatabase;
const directories: string[] = [];
const fixture = fs.readFileSync(new URL("./fixtures/knowledge-store-v18.sql", import.meta.url), "utf8");
const studioId = "fixture-v17-studio";
const researchTables = ["knowledge_research_runs", "knowledge_evidence_needs", "knowledge_research_rounds",
  "knowledge_research_read_receipts", "knowledge_evidence_items", "knowledge_need_evidence", "knowledge_research_actions"];

// 列顺序、类型、必填性、默认值和复合主键来自任务书 P3-02，不从被测建表代码生成期望值。
const columns = {
  knowledge_completeness_checks: [
    "id TEXT PK", "research_run_id TEXT NN", "policy TEXT NN", "status TEXT NN",
    "total_units INTEGER NN 0", "checked_units INTEGER NN 0", "relevant_units INTEGER NN 0",
    "unavailable_units INTEGER NN 0", "coverage_ratio REAL NN 0", "exact INTEGER NN 0",
    "created_at TEXT NN", "updated_at TEXT NN", "completed_at TEXT",
  ],
  knowledge_completeness_units: [
    "check_id TEXT NN PK1", "coverage_unit_id TEXT NN PK2", "source_id TEXT NN", "parse_artifact_id TEXT NN",
    "block_id TEXT NN", "start_offset INTEGER NN", "end_offset INTEGER NN", "section_key TEXT", "status TEXT NN",
    "worker_session_id TEXT", "updated_at TEXT NN",
  ],
  knowledge_completeness_unit_evidence: [
    "check_id TEXT NN PK1", "coverage_unit_id TEXT NN PK2", "evidence_id TEXT NN PK3",
  ],
  knowledge_completeness_coverage_runs: ["check_id TEXT NN PK1", "coverage_run_id TEXT NN PK2"],
};
const completenessTables = Object.keys(columns).sort();

function identifier(name: string) { return `"${name.replaceAll('"', '""')}"`; }
function tableNames(db: SqliteDatabase): string[] {
  return db.prepare<{ name: string }>("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
    .all().map((row: { name: string }) => row.name);
}
function schema(db: SqliteDatabase) {
  return db.prepare<SchemaEntry>("SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name").all();
}
function rows(db: SqliteDatabase, tables: string[]) {
  return Object.fromEntries(tables.map(table => [table, db.prepare(`SELECT * FROM ${identifier(table)} ORDER BY rowid`).all()]));
}
function databasePath() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-v19-migration-"));
  directories.push(directory);
  return path.join(directory, "knowledge.db");
}
function restoreV18() {
  const dbPath = databasePath();
  const db = new Database(dbPath);
  try {
    db.exec(fixture);
    expect(db.pragma("user_version", { simple: true })).toBe(18);
    expect(db.pragma("foreign_key_check")).toEqual([]);
    const tables = tableNames(db);
    expect(tables).toHaveLength(25);
    expect(tables.filter(table => completenessTables.includes(table))).toEqual([]);
    return { dbPath, tables, beforeRows: rows(db, tables), beforeSchema: schema(db) };
  } finally { db.close(); }
}
function assertCompletenessColumns(db: SqliteDatabase) {
  for (const [table, definitions] of Object.entries(columns)) {
    const expected = definitions.map(definition => {
      const [name, type, ...flags] = definition.split(" ");
      const primary = flags.find(flag => flag.startsWith("PK"));
      return { name, type, notnull: Number(flags.includes("NN") || Boolean(primary)), dflt_value: flags.includes("0") ? "0" : null,
        pk: primary ? Number(primary.slice(2) || 1) : 0 };
    });
    const actual = db.prepare<ColumnInfo>(`PRAGMA table_info(${identifier(table)})`).all()
      .map(({ name, type, notnull, dflt_value, pk }) => ({ name, type, notnull, dflt_value, pk }));
    expect(actual, table).toEqual(expected);
  }
  const indexes = db.prepare<{ name: string; unique: number }>("PRAGMA index_list(knowledge_completeness_checks)").all();
  expect(indexes.filter((index: { unique: number }) => index.unique === 1)
    .some((index: { name: string }) => {
      const indexed = db.prepare(`PRAGMA index_info(${identifier(index.name)})`).all();
      return indexed.length === 1 && indexed[0].name === "research_run_id";
    })).toBe(true);
}

afterEach(() => {
  for (const directory of directories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});

describe("Knowledge v18 → v19 真实旧库迁移", () => {
  it("fixture 固定来自 d1781134 的真实 v18，旧资料和七表研究记录都有数据", () => {
    expect(fixture).toContain("d17811341e0782d0f9190533dde366acb447a482");
    expect(crypto.createHash("sha256").update(fixture).digest("hex"))
      .toBe("017111b0ff49c2da4eb1f91a6d4a300dfa1762b7b6211c4dbe5c935ce34ab823");
    const { beforeRows } = restoreV18();
    expect(Object.values(beforeRows).reduce((count, entries) => count + entries.length, 0)).toBe(26);
    for (const table of ["notebooks", "sources", "notebook_sources", "content_snapshots", "parse_artifacts",
      "processing_artifacts", "knowledge_blocks", "knowledge_citations", "knowledge_turn_scopes",
      "knowledge_turn_scope_sources", "ingestion_jobs", ...researchTables]) {
      expect(beforeRows[table].length, table).toBeGreaterThan(0);
    }
    expect(beforeRows.knowledge_research_read_receipts[0].consumed_at).toBe("2026-09-04T06:00:00.000Z");
    expect(beforeRows.knowledge_evidence_items[0].canonical_text).toBe("项目 O'Reilly 在九月完成");
  });

  it("旧25表逐行及原结构不变，只新增四张指定表，重开幂等", () => {
    const { dbPath, tables, beforeRows, beforeSchema } = restoreV18();
    const store = new KnowledgeStore({ dbPath });
    let afterRows: ReturnType<typeof rows>, afterSchema: ReturnType<typeof schema>;
    try {
      expect(store.db.pragma("user_version", { simple: true })).toBe(19);
      expect(tableNames(store.db).filter(table => !tables.includes(table))).toEqual(completenessTables);
      expect(rows(store.db, tables)).toEqual(beforeRows);
      expect(schema(store.db).filter(entry => tables.includes(entry.tbl_name))).toEqual(beforeSchema);
      assertCompletenessColumns(store.db);
      expect(store.db.pragma("foreign_key_check")).toEqual([]);
      afterRows = rows(store.db, tableNames(store.db)); afterSchema = schema(store.db);
    } finally { store.close(); }
    const reopened = new KnowledgeStore({ dbPath });
    try {
      expect(reopened.db.pragma("user_version", { simple: true })).toBe(19);
      expect(rows(reopened.db, tableNames(reopened.db))).toEqual(afterRows);
      expect(schema(reopened.db)).toEqual(afterSchema);
    } finally { reopened.close(); }
  });

  it("空目录直接建成 v19，四张新表字段、默认值与主键和升级路径一致", () => {
    const dbPath = databasePath();
    const store = new KnowledgeStore({ dbPath });
    let createdSchema: ReturnType<typeof schema>;
    try {
      expect(store.db.pragma("user_version", { simple: true })).toBe(19);
      expect(tableNames(store.db)).toHaveLength(29);
      assertCompletenessColumns(store.db);
      for (const table of completenessTables) expect(store.db.prepare(`SELECT * FROM ${identifier(table)}`).all()).toEqual([]);
      expect(store.db.pragma("foreign_key_check")).toEqual([]);
      createdSchema = schema(store.db);
    } finally { store.close(); }
    const reopened = new KnowledgeStore({ dbPath });
    try {
      expect(reopened.db.pragma("user_version", { simple: true })).toBe(19);
      expect(schema(reopened.db)).toEqual(createdSchema);
      assertCompletenessColumns(reopened.db);
    } finally { reopened.close(); }
  });

  it("前两张表真实建立后由 SQLite 拒绝中段 DDL，回滚到原 v18 且可重试", () => {
    const { dbPath, tables, beforeRows, beforeSchema } = restoreV18();
    let partialTables: string[] = [], insideTransaction = false;
    class FailingDatabase extends Database {
      exec(sql: string) {
        const match = /CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?knowledge_completeness_unit_evidence\b/i.exec(sql);
        if (match) {
          // 先执行同一批中排在故障点之前的真实建表语句，确保失败发生于迁移中段。
          const preceding = sql.slice(0, match.index);
          if (preceding.trim()) super.exec(preceding);
          partialTables = tableNames(this).filter(table => completenessTables.includes(table));
          insideTransaction = this.inTransaction;
          // 同名列错误由 SQLite 自己抛出，不能在启动迁移前用模拟异常代替事务回滚。
          return super.exec("CREATE TABLE knowledge_migration_fault (id TEXT, id TEXT)");
        }
        return super.exec(sql);
      }
    }
    expect(() => {
      const unexpectedlyOpened = new KnowledgeStore({ dbPath, Database: FailingDatabase });
      unexpectedlyOpened.close();
    }).toThrow(/duplicate column name: id/i);
    expect(insideTransaction).toBe(true);
    expect(partialTables).toEqual(["knowledge_completeness_checks", "knowledge_completeness_units"]);
    const raw = new Database(dbPath);
    try {
      expect(raw.pragma("user_version", { simple: true })).toBe(18);
      expect(tableNames(raw)).toEqual(tables);
      expect(schema(raw)).toEqual(beforeSchema);
      expect(rows(raw, tables)).toEqual(beforeRows);
      expect(raw.pragma("foreign_key_check")).toEqual([]);
    } finally { raw.close(); }
    const retried = new KnowledgeStore({ dbPath });
    try {
      expect(retried.db.pragma("user_version", { simple: true })).toBe(19);
      assertCompletenessColumns(retried.db);
      expect(rows(retried.db, tables)).toEqual(beforeRows);
      expect(schema(retried.db).filter(entry => tables.includes(entry.tbl_name))).toEqual(beforeSchema);
    } finally { retried.close(); }
  });

  it("旧资料和研究记录经现有接口读取，已消费凭据与精确证据关系保持完整", () => {
    const { dbPath } = restoreV18();
    const store = new KnowledgeStore({ dbPath });
    try {
      expect(store.listNotebookSources({ studioId, notebookId: "nb_v17_002" })).toHaveLength(2);
      expect(store.resolveCitation({ studioId, citationId: "cite_v17_005" })).toMatchObject({
        citation: { canonicalText: "迁移前资料：项目 O'Reilly 在九月完成。", startOffset: 0, endOffset: 24 },
        source: { id: "src_v17_ready" }, snapshot: { id: "snap_v17_ready" }, artifact: { id: "parse_v17_ready", status: "ready" },
      });
      expect(store.getParseArtifact({ studioId, parseArtifactId: "parse_v17_ocr" })).toMatchObject({ status: "needs_ocr" });
      expect(store.getIngestionJob({ studioId, jobId: "ingjob_v17_006" }))
        .toMatchObject({ status: "running", progressDone: 1, progressTotal: 2 });
      const research = new ResearchStore(store);
      const runId = "krun_v18_001", needId = "kneed_v18_002", evidenceId = "kei_v18_005";
      expect(research.requireRun(runId)).toMatchObject({ status: "completed", stopReason: "complete", roundsCompleted: 1,
        turnScopeId: "kts_v17_007", completenessPolicy: "source_diverse" });
      expect(research.getNeed(runId, needId)).toMatchObject({ status: "supported", claim: "确定项目完成时间" });
      expect(research.listRounds(runId)).toMatchObject([{ id: "kround_v18_003", status: "completed", newEvidenceCount: 1 }]);
      expect(research.getReceipt(runId, "krr_v18_004")).toMatchObject({ sourceId: "src_v17_ready", contentSnapshotId: "snap_v17_ready",
        parseArtifactId: "parse_v17_ready", consumedAt: "2026-09-04T06:00:00.000Z", channel: "knowledge_read" });
      const evidence = research.listEvidence(runId);
      expect(evidence).toHaveLength(1);
      expect(evidence[0]).toMatchObject({ id: evidenceId, canonicalText: "项目 O'Reilly 在九月完成", startOffset: 6, endOffset: 23 });
      expect(evidence[0].canonicalTextSha256).toBe(crypto.createHash("sha256").update(evidence[0].canonicalText).digest("hex"));
      expect(research.listRelations(runId)).toMatchObject([{ needId, evidenceId, relation: "supports", sourceIndependenceKey: "src_v17_ready" }]);
      expect(research.listActions(runId)).toMatchObject([{ status: "completed", actionType: "knowledge_read",
        requestSummary: { sourceIds: ["src_v17_ready"], needIds: [needId] },
        responseSummary: { count: 1, receiptIds: ["krr_v18_004"] } }]);
    } finally { store.close(); }
  });
});
