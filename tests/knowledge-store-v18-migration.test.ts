import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, describe, expect, it } from "vitest";
import { KnowledgeStore } from "../lib/knowledge/knowledge-store.ts";

const Database = createRequire(import.meta.url)("better-sqlite3");
const directories: string[] = [];
const fixture = fs.readFileSync(new URL("./fixtures/knowledge-store-v17.sql", import.meta.url), "utf8");
const studioId = "fixture-v17-studio";

// 列顺序、类型、必填性、默认值和主键直接对应任务书 P2-02，不能从被测建表代码生成期望值。
const columns = {
  knowledge_research_runs: [
    "id TEXT PK", "turn_scope_id TEXT NN", "turn_id TEXT NN", "parent_session_path TEXT NN", "question TEXT NN",
    "status TEXT NN", "completeness_policy TEXT NN", "budget_json TEXT NN", "rounds_completed INTEGER NN 0",
    "tool_calls_used INTEGER NN 0", "search_calls INTEGER NN 0", "read_calls INTEGER NN 0", "grep_calls INTEGER NN 0",
    "delegated_agents INTEGER NN 0", "stop_reason TEXT", "degraded_reason TEXT", "created_at TEXT NN",
    "updated_at TEXT NN", "completed_at TEXT",
  ],
  knowledge_evidence_needs: [
    "id TEXT PK", "run_id TEXT NN", "ordinal INTEGER NN", "claim TEXT NN", "kind TEXT NN", "required INTEGER NN",
    "min_independent_sources INTEGER NN", "require_counter_evidence INTEGER NN", "require_all_relevant_units INTEGER NN",
    "status TEXT NN", "unresolved_gaps_json TEXT NN", "created_at TEXT NN", "updated_at TEXT NN",
  ],
  knowledge_research_rounds: [
    "id TEXT PK", "run_id TEXT NN", "ordinal INTEGER NN", "focus_json TEXT NN", "status TEXT NN",
    "new_evidence_count INTEGER NN 0", "started_at TEXT NN", "completed_at TEXT", "error_code TEXT",
  ],
  knowledge_research_read_receipts: [
    "id TEXT PK", "run_id TEXT NN", "actor_session_id TEXT", "source_id TEXT NN", "content_snapshot_id TEXT NN",
    "parse_artifact_id TEXT NN", "chunk_index_variant_id TEXT", "chunk_id TEXT", "block_id TEXT NN",
    "start_offset INTEGER NN", "end_offset INTEGER NN", "canonical_text_sha256 TEXT NN", "channel TEXT NN",
    "created_at TEXT NN", "consumed_at TEXT",
  ],
  knowledge_evidence_items: [
    "id TEXT PK", "run_id TEXT NN", "source_id TEXT NN", "content_snapshot_id TEXT NN", "parse_artifact_id TEXT NN",
    "chunk_index_variant_id TEXT", "chunk_id TEXT", "block_id TEXT NN", "start_offset INTEGER NN", "end_offset INTEGER NN",
    "canonical_text TEXT NN", "canonical_text_sha256 TEXT NN", "heading_path_json TEXT", "page_number INTEGER", "created_at TEXT NN",
  ],
  knowledge_need_evidence: [
    "need_id TEXT NN PK1", "evidence_id TEXT NN PK2", "relation TEXT NN PK3", "rationale TEXT NN",
    "source_independence_key TEXT NN", "created_at TEXT NN",
  ],
  knowledge_research_actions: [
    "id TEXT PK", "run_id TEXT NN", "round_id TEXT", "ordinal INTEGER NN", "actor_session_id TEXT", "actor_agent_id TEXT",
    "action_type TEXT NN", "request_summary_json TEXT NN", "response_summary_json TEXT", "status TEXT NN",
    "started_at TEXT NN", "completed_at TEXT", "error_code TEXT",
  ],
};
const researchTables = Object.keys(columns).sort();
// P3-02 只在原有七张研究表后增加这四张表，旧契约仍逐列验证。
const completenessColumns = {
  knowledge_completeness_checks: [
    "id TEXT PK", "research_run_id TEXT NN", "policy TEXT NN", "status TEXT NN", "total_units INTEGER NN 0",
    "checked_units INTEGER NN 0", "relevant_units INTEGER NN 0", "unavailable_units INTEGER NN 0",
    "coverage_ratio REAL NN 0", "exact INTEGER NN 0", "created_at TEXT NN", "updated_at TEXT NN", "completed_at TEXT",
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
const completenessTables = Object.keys(completenessColumns).sort();
const addedTables = [...researchTables, ...completenessTables].sort();

function identifier(name: string) { return `"${name.replaceAll('"', '""')}"`; }
function tableNames(db: any): string[] {
  return db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
    .all().map((row: { name: string }) => row.name);
}
function schema(db: any) {
  return db.prepare("SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name").all();
}
function rows(db: any, tables: string[]) {
  return Object.fromEntries(tables.map(table => [table, db.prepare(`SELECT * FROM ${identifier(table)} ORDER BY rowid`).all()]));
}
function databasePath() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-v18-migration-"));
  directories.push(directory);
  return path.join(directory, "knowledge.db");
}
function restoreV17() {
  const dbPath = databasePath();
  const db = new Database(dbPath);
  try {
    db.exec(fixture);
    expect(db.pragma("user_version", { simple: true })).toBe(17);
    expect(db.pragma("foreign_key_check")).toEqual([]);
    const tables = tableNames(db);
    expect(tables).toHaveLength(18);
    expect(tables.filter(table => addedTables.includes(table))).toEqual([]);
    return { dbPath, tables, beforeRows: rows(db, tables), beforeSchema: schema(db) };
  } finally { db.close(); }
}
function assertResearchColumns(db: any) {
  for (const [table, definitions] of Object.entries({ ...columns, ...completenessColumns })) {
    const expected = definitions.map(definition => {
      const [name, type, ...flags] = definition.split(" ");
      const primary = flags.find(flag => flag.startsWith("PK"));
      return { name, type, notnull: Number(flags.includes("NN") || Boolean(primary)), dflt_value: flags.includes("0") ? "0" : null,
        pk: primary ? Number(primary.slice(2) || 1) : 0 };
    });
    const actual = db.prepare(`PRAGMA table_info(${identifier(table)})`).all()
      .map(({ name, type, notnull, dflt_value, pk }: any) => ({ name, type, notnull, dflt_value, pk }));
    expect(actual, table).toEqual(expected);
  }
}

afterEach(() => {
  for (const directory of directories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});

describe("Knowledge v17 → v18 → v19 真实旧库完整迁移链", () => {
  it("fixture 固定来自 c3033b05 的真实 v17 建库，不以新库降低版本代替", () => {
    expect(fixture).toContain("c3033b05e09877bf425b3fd0e5ea9cf9b065c8da");
    expect(crypto.createHash("sha256").update(fixture).digest("hex"))
      .toBe("17c944f82151e23ca3b2f06d809256d4afa97db1225ba8e52677230640bb28d7");
    const { beforeRows } = restoreV17();
    expect(Object.values(beforeRows).reduce((count, entries) => count + entries.length, 0)).toBe(19);
    for (const table of ["notebooks", "sources", "notebook_sources", "content_snapshots", "parse_artifacts",
      "processing_artifacts", "knowledge_blocks", "knowledge_citations", "knowledge_turn_scopes", "knowledge_turn_scope_sources", "ingestion_jobs"]) {
      expect(beforeRows[table].length, table).toBeGreaterThan(0);
    }
  });

  it("全部旧表逐行及原结构不变，v18 七表外只追加 v19 四表，重开幂等", () => {
    const { dbPath, tables, beforeRows, beforeSchema } = restoreV17();
    const store = new KnowledgeStore({ dbPath });
    let afterRows: ReturnType<typeof rows>, afterSchema: ReturnType<typeof schema>;
    try {
      expect(store.db.pragma("user_version", { simple: true })).toBe(19);
      expect(tableNames(store.db).filter(table => !tables.includes(table))).toEqual(addedTables);
      expect(tableNames(store.db)).toHaveLength(29);
      expect(rows(store.db, tables)).toEqual(beforeRows);
      expect(schema(store.db).filter((entry: any) => tables.includes(entry.tbl_name))).toEqual(beforeSchema);
      assertResearchColumns(store.db);
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

  it("空目录直接建成 v19，原七张研究表与新增四张完整性表字段都与升级路径一致", () => {
    const { tables } = restoreV17();
    const store = new KnowledgeStore({ dbPath: databasePath() });
    try {
      expect(store.db.pragma("user_version", { simple: true })).toBe(19);
      expect(tableNames(store.db)).toEqual([...tables, ...addedTables].sort());
      assertResearchColumns(store.db);
      for (const table of addedTables) expect(store.db.prepare(`SELECT * FROM ${identifier(table)}`).all()).toEqual([]);
      expect(store.db.pragma("foreign_key_check")).toEqual([]);
    } finally { store.close(); }
  });

  it("真实执行 v18 前三张新表后让中段 DDL 报错，整个迁移回滚且重试可升级至 v19", () => {
    const { dbPath, tables, beforeRows, beforeSchema } = restoreV17();
    let partialTables: string[] = [], insideTransaction = false;
    class FailingDatabase extends Database {
      exec(sql: string) {
        const match = /CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?knowledge_research_read_receipts\b/i.exec(sql);
        if (match) {
          // 先真实执行同一次 exec 中已经排在前面的建表语句，不能在迁移开始前直接抛异常。
          const preceding = sql.slice(0, match.index);
          if (preceding.trim()) super.exec(preceding);
          partialTables = tableNames(this).filter(table => researchTables.includes(table));
          insideTransaction = this.inTransaction;
          // 同名列由 SQLite 本身拒绝，模拟中段建表失败；此前执行过的表必须随外层事务回滚。
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
    expect(partialTables).toEqual(["knowledge_evidence_needs", "knowledge_research_rounds", "knowledge_research_runs"]);
    const raw = new Database(dbPath);
    try {
      expect(raw.pragma("user_version", { simple: true })).toBe(17);
      expect(tableNames(raw)).toEqual(tables);
      expect(schema(raw)).toEqual(beforeSchema);
      expect(rows(raw, tables)).toEqual(beforeRows);
      expect(raw.pragma("foreign_key_check")).toEqual([]);
    } finally { raw.close(); }
    const retried = new KnowledgeStore({ dbPath });
    try {
      expect(retried.db.pragma("user_version", { simple: true })).toBe(19);
      expect(tableNames(retried.db)).toEqual([...tables, ...addedTables].sort());
      assertResearchColumns(retried.db);
      expect(rows(retried.db, tables)).toEqual(beforeRows);
      expect(schema(retried.db).filter((entry: any) => tables.includes(entry.tbl_name))).toEqual(beforeSchema);
      expect(retried.db.pragma("foreign_key_check")).toEqual([]);
    } finally { retried.close(); }
  });

  it("v18 七表实际执行完成后再于 v19 中段失败，整条迁移链回滚到原始 v17", () => {
    const { dbPath, tables, beforeRows, beforeSchema } = restoreV17();
    let partialTables: string[] = [], insideTransaction = false;
    class FailingDatabase extends Database {
      exec(sql: string) {
        const match = /CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?knowledge_completeness_unit_evidence\b/i.exec(sql);
        if (match) {
          // 前一步七张研究表与本步骤前两张表都必须真实建成，再触发 SQLite 本身的 DDL 错误。
          const preceding = sql.slice(0, match.index);
          if (preceding.trim()) super.exec(preceding);
          partialTables = tableNames(this).filter(table => addedTables.includes(table));
          insideTransaction = this.inTransaction;
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
    expect(partialTables).toEqual([
      ...researchTables, "knowledge_completeness_checks", "knowledge_completeness_units",
    ].sort());
    const raw = new Database(dbPath);
    try {
      expect(raw.pragma("user_version", { simple: true })).toBe(17);
      expect(tableNames(raw)).toEqual(tables);
      expect(schema(raw)).toEqual(beforeSchema);
      expect(rows(raw, tables)).toEqual(beforeRows);
      expect(raw.pragma("foreign_key_check")).toEqual([]);
    } finally { raw.close(); }
    const retried = new KnowledgeStore({ dbPath });
    try {
      expect(retried.db.pragma("user_version", { simple: true })).toBe(19);
      expect(tableNames(retried.db)).toEqual([...tables, ...addedTables].sort());
      assertResearchColumns(retried.db);
      expect(rows(retried.db, tables)).toEqual(beforeRows);
      expect(schema(retried.db).filter((entry: any) => tables.includes(entry.tbl_name))).toEqual(beforeSchema);
      expect(retried.db.pragma("foreign_key_check")).toEqual([]);
    } finally { retried.close(); }
  });

  it("老库通过现有接口继续读取共享源、精确引用、扫描状态和摄入进度", () => {
    const { dbPath } = restoreV17();
    const store = new KnowledgeStore({ dbPath });
    try {
      expect(store.getNotebook({ studioId, notebookId: "nb_v17_001" }).name).toBe("迁移前的甲资料");
      expect(store.getNotebookConfig({ studioId, notebookId: "nb_v17_001" })).toMatchObject({
        embeddingModelRef: { provider: "fixture-provider", id: "fixture-embedding" }, chunkTargetChars: 512, vectorRetentionDays: 90,
      });
      expect(store.listNotebookSources({ studioId, notebookId: "nb_v17_002" })).toHaveLength(2);
      expect(store.resolveCitation({ studioId, citationId: "cite_v17_005" })).toMatchObject({
        citation: { canonicalText: "迁移前资料：项目 O'Reilly 在九月完成。", startOffset: 0, endOffset: 24 },
        source: { id: "src_v17_ready" }, snapshot: { id: "snap_v17_ready" }, artifact: { id: "parse_v17_ready", status: "ready" },
      });
      expect(store.getParseArtifact({ studioId, parseArtifactId: "parse_v17_ocr" })).toMatchObject({ status: "needs_ocr" });
      expect(store.getTurnScope({ scopeId: "kts_v17_007" })).toMatchObject({
        notebookIds: ["nb_v17_001", "nb_v17_002"], sources: expect.arrayContaining([
          expect.objectContaining({ sourceId: "src_v17_ready", notebookIds: ["nb_v17_001", "nb_v17_002"] }),
        ]),
      });
      expect(store.getIngestionJob({ studioId, jobId: "ingjob_v17_006" }))
        .toMatchObject({ status: "running", progressDone: 1, progressTotal: 2 });
    } finally { store.close(); }
  });
});
