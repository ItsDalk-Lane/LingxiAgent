import crypto from "node:crypto";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { KnowledgeStore } from "../lib/knowledge/knowledge-store.ts";
import { createKnowledgeResearchFixture } from "./helpers/knowledge-research-fixture.ts";

const TABLES = {
  checks: "knowledge_completeness_checks",
  units: "knowledge_completeness_units",
  evidence: "knowledge_completeness_unit_evidence",
  coverage: "knowledge_completeness_coverage_runs",
} as const;
type Table = keyof typeof TABLES;
type Row = Record<string, string | number | null>;
const NOW = "2026-09-04T00:00:00.000Z";
const fixtures: ReturnType<typeof createKnowledgeResearchFixture>[] = [];
const reopened: KnowledgeStore[] = [];
afterEach(() => {
  for (const store of reopened.splice(0)) store.close();
  for (const fixture of fixtures.splice(0)) fixture.close();
});

/** 新表直接使用SQL写入，防止应用层校验替代对数据库约束的验证。 */
function fixture() {
  const f = createKnowledgeResearchFixture(); fixtures.push(f);
  const source = f.sources[0];
  const evidence = [0, 5].map(startOffset => {
    const endOffset = startOffset + 5, canonicalText = source.text.slice(startOffset, endOffset);
    return f.research.putEvidence({ id: `evidence-${startOffset}`, runId: f.run.id,
      sourceId: source.sourceId, contentSnapshotId: source.contentSnapshotId,
      parseArtifactId: source.parseArtifactId, blockId: source.blockId,
      chunkIndexVariantId: null, chunkId: null, startOffset, endOffset, canonicalText,
      canonicalTextSha256: crypto.createHash("sha256").update(canonicalText).digest("hex"),
      headingPath: null, pageNumber: null, createdAt: NOW });
  });
  f.store.db.prepare(`INSERT INTO coverage_runs (id, turn_scope_id, manifest_hash, manifest_json,
    status, expected_units, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`)
    .run("coverage-1", f.scope.id, "a".repeat(64), "{}", "pending", 1, NOW, NOW);
  const rows: Record<Table, Row> = {
    checks: { id: "check-1", research_run_id: f.run.id, policy: "scope_complete", status: "pending",
      total_units: 0, checked_units: 0, relevant_units: 0, unavailable_units: 0,
      coverage_ratio: 0, exact: 0, created_at: NOW, updated_at: NOW, completed_at: null },
    units: { check_id: "check-1", coverage_unit_id: "unit-1", source_id: source.sourceId,
      parse_artifact_id: source.parseArtifactId, block_id: source.blockId, start_offset: 0,
      end_offset: source.text.length, section_key: null, status: "pending", worker_session_id: null, updated_at: NOW },
    evidence: { check_id: "check-1", coverage_unit_id: "unit-1", evidence_id: evidence[0].id },
    coverage: { check_id: "check-1", coverage_run_id: "coverage-1" },
  };
  function insert(table: Table, row: Row) {
    const columns = Object.keys(row);
    return f.store.db.prepare(`INSERT INTO ${TABLES[table]} (${columns.join(",")}) VALUES (${columns.map(() => "?").join(",")})`)
      .run(...Object.values(row));
  }
  function update(table: Table, changes: Row) {
    const key = table === "checks" ? "id = 'check-1'" : "check_id = 'check-1'";
    return f.store.db.prepare(`UPDATE ${TABLES[table]} SET ${Object.keys(changes).map(column => `${column} = ?`).join(",")} WHERE ${key}`)
      .run(...Object.values(changes));
  }
  function anotherCheck() {
    const run = f.research.createRun({ turnScopeId: f.scope.id, turnId: f.scope.turnId,
      parentSessionPath: f.scope.sessionPath, question: "另一轮完整性检查" });
    insert("checks", { ...rows.checks, id: "check-2", research_run_id: run.id });
  }
  for (const table of Object.keys(TABLES) as Table[]) insert(table, rows[table]);
  return { ...f, rows, evidence, insert, update, anotherCheck };
}

describe("完整性检查四表的真实SQLite持久化约束", () => {
  it("一轮研究只允许一个检查，检查和单元按组合唯一，不把单元编号错误锁为全局唯一", () => {
    const f = fixture();
    expect(() => f.insert("checks", { ...f.rows.checks, id: "check-duplicate" })).toThrow(/UNIQUE constraint failed/);
    expect(() => f.insert("units", f.rows.units)).toThrow(/UNIQUE constraint failed/);
    f.anotherCheck();
    expect(() => f.insert("units", { ...f.rows.units, check_id: "check-2" })).not.toThrow();
    expect(f.store.db.prepare(`SELECT COUNT(*) AS count FROM ${TABLES.units}`).get().count).toBe(2);
  });

  it("同一单元可关联多个证据，三元关联与检查/coverage运行的复合关联均不能重复", () => {
    const f = fixture();
    f.insert("evidence", { ...f.rows.evidence, evidence_id: f.evidence[1].id });
    expect(f.store.db.prepare(`SELECT COUNT(*) AS count FROM ${TABLES.evidence}`).get().count).toBe(2);
    expect(() => f.insert("evidence", f.rows.evidence)).toThrow(/UNIQUE constraint failed/);
    f.insert("units", { ...f.rows.units, coverage_unit_id: "unit-2" });
    expect(() => f.insert("evidence", { ...f.rows.evidence, coverage_unit_id: "unit-2" })).not.toThrow();
    expect(() => f.insert("coverage", f.rows.coverage)).toThrow(/UNIQUE constraint failed/);
    f.store.db.prepare(`INSERT INTO coverage_runs (id, turn_scope_id, manifest_hash, manifest_json,
      status, expected_units, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`)
      .run("coverage-2", f.scope.id, "b".repeat(64), "{}", "pending", 1, NOW, NOW);
    expect(() => f.insert("coverage", { ...f.rows.coverage, coverage_run_id: "coverage-2" })).not.toThrow();
    f.anotherCheck();
    expect(() => f.insert("coverage", { ...f.rows.coverage, check_id: "check-2" })).not.toThrow();
  });

  it.each([
    ["checks", "research_run_id"], ["units", "check_id"], ["units", "source_id"],
    ["units", "parse_artifact_id"], ["units", "block_id"], ["evidence", "check_id"],
    ["evidence", "coverage_unit_id"], ["evidence", "evidence_id"],
    ["coverage", "check_id"], ["coverage", "coverage_run_id"],
  ] as const)("%s.%s不能引用不存在的记录，新增和改写都被外键拒绝", (table, column) => {
    const f = fixture();
    const changed = { ...f.rows[table], [column]: "missing-reference" };
    if (table === "checks") changed.id = "check-missing-run";
    if (table === "units") changed.coverage_unit_id = "unit-missing-reference";
    expect(() => f.insert(table, changed)).toThrow(/FOREIGN KEY constraint failed/);
    expect(() => f.update(table, { [column]: "missing-reference" })).toThrow(/FOREIGN KEY constraint failed/);
  });

  it("检查和单元各自存在仍不足以关联证据，两者必须属于同一个复合身份", () => {
    const f = fixture(); f.anotherCheck();
    expect(() => f.insert("evidence", { ...f.rows.evidence, check_id: "check-2" })).toThrow(/FOREIGN KEY constraint failed/);
    f.insert("units", { ...f.rows.units, check_id: "check-2", coverage_unit_id: "unit-2" });
    expect(() => f.insert("evidence", { ...f.rows.evidence, coverage_unit_id: "unit-2" })).toThrow(/FOREIGN KEY constraint failed/);
  });

  it("省略计数字段时使用零默认值，检查状态只要求非空而不擅自限定枚举", () => {
    const f = fixture();
    f.store.db.prepare(`DELETE FROM ${TABLES.coverage}`).run();
    f.store.db.prepare(`DELETE FROM ${TABLES.evidence}`).run();
    f.store.db.prepare(`DELETE FROM ${TABLES.units}`).run();
    f.store.db.prepare(`DELETE FROM ${TABLES.checks}`).run();
    f.insert("checks", { id: "check-1", research_run_id: f.run.id, policy: "best_effort",
      status: "awaiting_host_verification", created_at: NOW, updated_at: NOW });
    expect(f.store.db.prepare(`SELECT * FROM ${TABLES.checks}`).get()).toMatchObject({
      total_units: 0, checked_units: 0, relevant_units: 0, unavailable_units: 0,
      coverage_ratio: 0, exact: 0, completed_at: null, status: "awaiting_host_verification",
    });
    for (const status of ["", "   ", null]) expect(() => f.update("checks", { status })).toThrow(/constraint failed/);
  });

  it("策略只允许既有四档，单元状态只允许任务书锁定的五档", () => {
    const f = fixture();
    for (const policy of ["best_effort", "source_diverse", "relevant_sections_complete", "scope_complete"]) {
      expect(() => f.update("checks", { policy })).not.toThrow();
    }
    for (const policy of ["unknown", "", null]) expect(() => f.update("checks", { policy })).toThrow(/constraint failed/);
    for (const status of ["pending", "checked_relevant", "checked_irrelevant", "unavailable", "failed"]) {
      expect(() => f.update("units", { status })).not.toThrow();
    }
    for (const status of ["checked", "completed", "", null]) expect(() => f.update("units", { status })).toThrow(/constraint failed/);
  });

  it.each(["total_units", "checked_units", "relevant_units", "unavailable_units"])("%s必须是非负整数", column => {
    const f = fixture();
    f.update("checks", { total_units: 10, checked_units: column === "relevant_units" ? 5 : 0 });
    for (const value of [-1, 1.5, "not-a-count", null]) {
      expect(() => f.update("checks", { [column]: value })).toThrow(/constraint failed/);
    }
    expect(() => f.update("checks", { [column]: 0 })).not.toThrow();
  });

  it("已检查与不可用数量之和不能超过总数，相关数量不能超过已检查数量", () => {
    const f = fixture();
    expect(() => f.update("checks", { total_units: 2, checked_units: 2, unavailable_units: 1 })).toThrow(/constraint failed/);
    expect(() => f.update("checks", { total_units: 3, checked_units: 1, relevant_units: 2 })).toThrow(/constraint failed/);
    expect(() => f.update("checks", { total_units: 3, checked_units: 2, relevant_units: 2, unavailable_units: 1 })).not.toThrow();
  });

  it("偏移只能使用非负整数且结束位置严格大于起点", () => {
    const f = fixture();
    for (const changes of [{ start_offset: -1 }, { start_offset: 0.5 }, { start_offset: "bad" }, { start_offset: null },
      { end_offset: -1 }, { end_offset: 0 }, { end_offset: 0.5 }, { end_offset: "bad" }, { end_offset: null },
      { start_offset: 2, end_offset: 1 }, { start_offset: 2, end_offset: 2 }]) {
      expect(() => f.update("units", changes)).toThrow(/constraint failed/);
    }
    expect(() => f.update("units", { start_offset: 0, end_offset: 1 })).not.toThrow();
  });

  it("覆盖率只能位于零到一，exact只接受布尔标记及全量已检查的真实计数", () => {
    const f = fixture();
    for (const coverage_ratio of [-0.01, 1.01, "bad", null]) expect(() => f.update("checks", { coverage_ratio })).toThrow(/constraint failed/);
    for (const coverage_ratio of [0, 0.5, 1]) expect(() => f.update("checks", { coverage_ratio })).not.toThrow();
    f.update("checks", { total_units: 2, checked_units: 2, relevant_units: 1, coverage_ratio: 1 });
    for (const exact of [-1, 2, 0.5, "bad", null]) expect(() => f.update("checks", { exact })).toThrow(/constraint failed/);
    expect(() => f.update("checks", { exact: 1 })).not.toThrow();
    expect(() => f.update("checks", { checked_units: 1 })).toThrow(/constraint failed/);
    expect(() => f.update("checks", { checked_units: 1, unavailable_units: 1 })).toThrow(/constraint failed/);
    expect(() => f.update("checks", { coverage_ratio: 0.99 })).toThrow(/constraint failed/);
    expect(() => f.update("checks", { exact: 0, checked_units: 1, unavailable_units: 1, coverage_ratio: 0.5 })).not.toThrow();
  });

  it("关闭并重新打开数据库后，四张表的计数、状态、定位和多证据关联完整保留", () => {
    const f = fixture();
    f.update("checks", { total_units: 1, checked_units: 1, relevant_units: 1, coverage_ratio: 1, exact: 1,
      status: "host_verified", completed_at: NOW });
    f.update("units", { status: "checked_relevant", section_key: "section-1", worker_session_id: "worker-preserved" });
    f.insert("evidence", { ...f.rows.evidence, evidence_id: f.evidence[1].id });
    const snapshot = Object.fromEntries(Object.entries(TABLES).map(([key, table]) => [key, f.store.db.prepare(`SELECT * FROM ${table} ORDER BY 1, 2`).all()]));
    const dbPath = path.join(path.dirname(f.scope.sessionPath), "knowledge.db");
    f.store.close();
    const restored = new KnowledgeStore({ dbPath }); reopened.push(restored);
    for (const [key, table] of Object.entries(TABLES)) {
      expect(restored.db.prepare(`SELECT * FROM ${table} ORDER BY 1, 2`).all()).toEqual(snapshot[key]);
    }
    expect(restored.db.pragma("foreign_key_check")).toEqual([]);
    expect(restored.db.pragma("foreign_keys", { simple: true })).toBe(1);
  });
});
