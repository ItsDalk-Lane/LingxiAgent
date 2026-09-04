import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeStore } from "../lib/knowledge/knowledge-store.ts";
import { DEFAULT_KNOWLEDGE_RESEARCH_BUDGET } from "../shared/knowledge-research.ts";

const TABLES = {
  runs: "knowledge_research_runs",
  needs: "knowledge_evidence_needs",
  rounds: "knowledge_research_rounds",
  receipts: "knowledge_research_read_receipts",
  evidence: "knowledge_evidence_items",
  relations: "knowledge_need_evidence",
  actions: "knowledge_research_actions",
} as const;
type TableKey = keyof typeof TABLES;
type Row = Record<string, string | number | null>;
const roots: string[] = [];
const stores: KnowledgeStore[] = [];
const NOW = "2026-09-04T00:00:00.000Z";
const TEXT = "交付日期是九月十五日。";
const HASH = crypto.createHash("sha256").update(TEXT).digest("hex");
const NULLABLE: Record<TableKey, readonly string[]> = {
  runs: ["stop_reason", "degraded_reason", "completed_at"],
  needs: [],
  rounds: ["completed_at", "error_code"],
  receipts: ["actor_session_id", "chunk_index_variant_id", "chunk_id", "consumed_at"],
  evidence: ["chunk_index_variant_id", "chunk_id", "heading_path_json", "page_number"],
  relations: [],
  actions: ["round_id", "actor_session_id", "actor_agent_id", "response_summary_json", "completed_at", "error_code"],
};

afterEach(() => {
  for (const store of stores.splice(0)) store.close();
  for (const root of roots.splice(0)) {
    fs.rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});

function openStore(dbPath: string) {
  const store = new KnowledgeStore({ dbPath, now: () => NOW });
  stores.push(store);
  return store;
}

function insert(store: KnowledgeStore, table: TableKey, row: Row) {
  const columns = Object.keys(row);
  return store.db.prepare(`
    INSERT INTO ${TABLES[table]} (${columns.join(", ")})
    VALUES (${columns.map(() => "?").join(", ")})
  `).run(...Object.values(row));
}

/** 先建立真实资料身份链，再直接写数据库，避免应用层预校验遮住数据库约束。 */
function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-research-store-"));
  roots.push(root);
  const dbPath = path.join(root, "knowledge.db");
  const store = openStore(dbPath);
  const studioId = "research-store-test";
  const notebook = store.createNotebook({ studioId, name: "研究资料" });
  const source = store.createSourceWithSnapshot({
    studioId,
    notebookId: notebook.id,
    sourceType: "pasted_text",
    displayName: "项目进度",
    originMetadata: {},
    snapshot: {
      sha256: HASH,
      mimeType: "text/plain",
      byteSize: Buffer.byteLength(TEXT),
      storagePath: "sources/research-test/snapshot.txt",
    },
  });
  const artifact = store.beginParseArtifact({
    studioId,
    contentSnapshotId: source.snapshot.id,
    parserId: "text",
    parserVersion: "1",
    parserConfigHash: "a".repeat(64),
  });
  store.completeParseArtifact({
    studioId,
    parseArtifactId: artifact.id,
    status: "ready",
    warnings: [],
    semanticArtifactPath: "parsed/research-test.txt",
    blocks: [{
      ordinal: 0,
      text: TEXT,
      locatorType: "text",
      locator: { lineStart: 1, lineEnd: 1, charStart: 0, charEnd: TEXT.length },
    }],
  });
  const block = store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id })[0];
  const scope = store.createTurnScope({
    studioId,
    notebookIds: [notebook.id],
    sessionPath: path.join(root, "parent.jsonl"),
    turnId: "turn-1",
  });
  const location = {
    source_id: source.source.id,
    content_snapshot_id: source.snapshot.id,
    parse_artifact_id: artifact.id,
    chunk_index_variant_id: null,
    chunk_id: null,
    block_id: block.id,
    start_offset: 0,
    end_offset: TEXT.length,
    canonical_text_sha256: HASH,
  };
  const rows: Record<TableKey, Row> = {
    runs: {
      id: "run-1", turn_scope_id: scope.id, turn_id: "turn-1",
      parent_session_path: scope.sessionPath, question: "什么时候交付？",
      status: "planning", completeness_policy: "source_diverse",
      budget_json: JSON.stringify(DEFAULT_KNOWLEDGE_RESEARCH_BUDGET),
      rounds_completed: 0, tool_calls_used: 0, search_calls: 0, read_calls: 0,
      grep_calls: 0, delegated_agents: 0, stop_reason: null, degraded_reason: null,
      created_at: NOW, updated_at: NOW, completed_at: null,
    },
    needs: {
      id: "need-1", run_id: "run-1", ordinal: 0, claim: "确认交付时间", kind: "fact",
      required: 1, min_independent_sources: 1, require_counter_evidence: 0,
      require_all_relevant_units: 0, status: "uncovered", unresolved_gaps_json: "[]",
      created_at: NOW, updated_at: NOW,
    },
    rounds: {
      id: "round-1", run_id: "run-1", ordinal: 0, focus_json: '["need-1"]',
      status: "running", new_evidence_count: 0, started_at: NOW,
      completed_at: null, error_code: null,
    },
    receipts: {
      id: "receipt-1", run_id: "run-1", actor_session_id: null, ...location,
      channel: "knowledge_read", created_at: NOW, consumed_at: null,
    },
    evidence: {
      id: "evidence-1", run_id: "run-1", ...location, canonical_text: TEXT,
      heading_path_json: '["项目进度"]', page_number: 1, created_at: NOW,
    },
    relations: {
      need_id: "need-1", evidence_id: "evidence-1", relation: "supports",
      rationale: "原文直接给出日期", source_independence_key: source.source.id, created_at: NOW,
    },
    actions: {
      id: "action-1", run_id: "run-1", round_id: "round-1", ordinal: 0,
      actor_session_id: null, actor_agent_id: null, action_type: "knowledge_read",
      request_summary_json: '{"needIds":["need-1"]}',
      response_summary_json: '{"receiptIds":["receipt-1"],"count":1}',
      status: "completed", started_at: NOW, completed_at: NOW, error_code: null,
    },
  };
  for (const table of Object.keys(TABLES) as TableKey[]) insert(store, table, rows[table]);
  function change(table: TableKey, column: string, value: Row[string]) {
    const where = table === "relations"
      ? "need_id = 'need-1' AND evidence_id = 'evidence-1'"
      : `id = '${rows[table].id}'`;
    return store.db.prepare(`UPDATE ${TABLES[table]} SET ${column} = ? WHERE ${where}`).run(value);
  }
  function reject(table: TableKey, column: string, values: Row[string][], error = /CHECK constraint failed/) {
    for (const value of values) {
      expect(() => change(table, column, value), `${TABLES[table]}.${column} = ${String(value)}`).toThrow(error);
    }
  }
  function accept(table: TableKey, column: string, values: Row[string][]) {
    for (const value of values) {
      expect(change(table, column, value).changes).toBe(1);
    }
    change(table, column, rows[table][column]);
  }
  return { store, dbPath, rows, change, reject, accept, source, artifact, block, scope };
}

describe("知识研究 v18 数据库约束", () => {
  it("七张表严格保留任务书列；所有必填列（含主键）拒绝 NULL", () => {
    const { store, rows, reject, accept } = fixture();
    for (const table of Object.keys(TABLES) as TableKey[]) {
      const columns = store.db.pragma(`table_info(${TABLES[table]})`) as Array<{ name: string; type: string; notnull: number; pk: number }>;
      expect(columns.map(column => column.name).sort()).toEqual(Object.keys(rows[table]).sort());
      expect(columns.filter(column => column.pk > 0).sort((a, b) => a.pk - b.pk).map(column => column.name))
        .toEqual(table === "relations" ? ["need_id", "evidence_id", "relation"] : ["id"]);
      for (const column of columns) {
        expect(column.type, `${TABLES[table]}.${column.name}`).toBe(
          typeof rows[table][column.name] === "number" ? "INTEGER" : "TEXT",
        );
        if (NULLABLE[table].includes(column.name)) {
          accept(table, column.name, [null]);
        } else {
          expect(column.notnull, `${TABLES[table]}.${column.name}`).toBe(1);
          reject(table, column.name, [null], /NOT NULL constraint failed/);
        }
      }
    }
  });

  it("研究运行只接受七种状态和四种完整性策略", () => {
    const { accept, reject } = fixture();
    accept("runs", "status", ["planning", "running", "synthesizing", "completed", "partial", "failed", "cancelled"]);
    reject("runs", "status", ["done", "timeout", "", "RUNNING"]);
    accept("runs", "completeness_policy", ["best_effort", "source_diverse", "relevant_sections_complete", "scope_complete"]);
    reject("runs", "completeness_policy", ["exhaustive", "fast", "", "BEST_EFFORT"]);
  });

  it("必填身份、问题、主张、理由和动作名称不能只有空格，证据正文不能为空", () => {
    const { reject } = fixture();
    const fields: Array<[TableKey, string[]]> = [
      ["runs", ["id", "turn_id", "parent_session_path", "question"]],
      ["needs", ["id", "claim"]], ["rounds", ["id"]], ["receipts", ["id"]],
      ["evidence", ["id"]], ["relations", ["rationale"]], ["actions", ["id", "action_type"]],
    ];
    for (const [table, columns] of fields) {
      for (const column of columns) reject(table, column, ["", "   "]);
    }
    reject("evidence", "canonical_text", [""]);
  });

  it("需求只接受六种类型、五种状态；关联只接受三种关系", () => {
    const { accept, reject } = fixture();
    accept("needs", "kind", ["fact", "comparison", "cause", "timeline", "counterexample", "completeness"]);
    reject("needs", "kind", ["claim", "", "FACT"]);
    accept("needs", "status", ["uncovered", "partial", "supported", "conflicted", "not_applicable"]);
    reject("needs", "status", ["completed", "failed", "", "SUPPORTED"]);
    accept("relations", "relation", ["supports", "contradicts", "context"]);
    reject("relations", "relation", ["support", "opposes", "", "SUPPORTS"]);
  });

  it("研究轮次、动作状态和读取回执渠道拒绝未知值", () => {
    const { accept, reject } = fixture();
    for (const table of ["rounds", "actions"] as const) {
      accept(table, "status", ["running", "completed", "failed", "cancelled"]);
      reject(table, "status", ["pending", "success", "", "RUNNING"]);
    }
    accept("receipts", "channel", ["knowledge_read", "knowledge_grep"]);
    reject("receipts", "channel", ["knowledge_search", "knowledge_outline", "", "read"]);
  });

  it("计数和顺序必须是非负整数，零值仍是合法值", () => {
    const { accept, reject } = fixture();
    const columns: Array<[TableKey, string[]]> = [
      ["runs", ["rounds_completed", "tool_calls_used", "search_calls", "read_calls", "grep_calls", "delegated_agents"]],
      ["needs", ["ordinal"]], ["rounds", ["ordinal", "new_evidence_count"]], ["actions", ["ordinal"]],
    ];
    for (const [table, names] of columns) {
      for (const column of names) {
        reject(table, column, [-1, 0.5, "not-a-number", 1e30]);
        accept(table, column, [0, 3]);
      }
    }
  });

  it("需求布尔开关只接受零或一，独立来源数必须是正整数", () => {
    const { accept, reject } = fixture();
    for (const column of ["required", "require_counter_evidence", "require_all_relevant_units"]) {
      accept("needs", column, [0, 1]);
      reject("needs", column, [-1, 2, 0.5, "true", "false"]);
    }
    accept("needs", "min_independent_sources", [1, 2]);
    reject("needs", "min_independent_sources", [0, -1, 1.5, "many", 1e30]);
  });

  it("读取和证据位置必须是非负整数且结束位置大于开始位置", () => {
    const { accept, reject, change } = fixture();
    for (const table of ["receipts", "evidence"] as const) {
      reject(table, "start_offset", [-1, 0.5, "start", 1e30, TEXT.length, TEXT.length + 1]);
      reject(table, "end_offset", [-1, 0, 0.5, "end", 1e30]);
      accept(table, "start_offset", [0, 1]);
      change(table, "start_offset", 2);
      reject(table, "end_offset", [1, 2]);
      change(table, "start_offset", 0);
    }
    accept("evidence", "page_number", [null, 1, 2]);
    reject("evidence", "page_number", [0, -1, 1.5, "page", 1e30]);
  });

  it("原文摘要只接受六十四位小写十六进制，不把长度检查当成摘要验证", () => {
    const { accept, reject } = fixture();
    for (const table of ["receipts", "evidence"] as const) {
      accept(table, "canonical_text_sha256", [HASH, "0".repeat(64), "abcdef0123456789".repeat(4)]);
      reject(table, "canonical_text_sha256", ["a".repeat(63), "a".repeat(65), "A".repeat(64), "g".repeat(64), `${"a".repeat(63)}\n`, `${"a".repeat(64)}\0hidden`, ""]);
    }
  });

  it("所有 JSON 列拒绝损坏文档及对象、数组形状混用", () => {
    const { accept, reject } = fixture();
    const columns: Array<[TableKey, string, "object" | "array"]> = [
      ["runs", "budget_json", "object"], ["needs", "unresolved_gaps_json", "array"],
      ["rounds", "focus_json", "array"], ["evidence", "heading_path_json", "array"],
      ["actions", "request_summary_json", "object"], ["actions", "response_summary_json", "object"],
    ];
    for (const [table, column, shape] of columns) {
      reject(table, column, ["{", "[1,", "undefined", "", "null", "false", "42", '"text"', shape === "object" ? "[]" : "{}"], /CHECK constraint failed|malformed JSON/);
      if (table !== "runs") accept(table, column, shape === "object" ? ["{}", '{"count":1}'] : ["[]", '["待补证据"]']);
    }
  });

  it("预算对象八个字段缺一不可，每个字段都必须是正整数", () => {
    const { accept, reject } = fixture();
    reject("runs", "budget_json", ["{}"]);
    for (const field of Object.keys(DEFAULT_KNOWLEDGE_RESEARCH_BUDGET)) {
      const missing: Record<string, unknown> = { ...DEFAULT_KNOWLEDGE_RESEARCH_BUDGET };
      delete missing[field];
      reject("runs", "budget_json", [JSON.stringify(missing)]);
      for (const invalid of [0, -1, 0.5, null, "4", true, [], {}, 1e30]) {
        reject("runs", "budget_json", [JSON.stringify({ ...DEFAULT_KNOWLEDGE_RESEARCH_BUDGET, [field]: invalid })]);
      }
      accept("runs", "budget_json", [JSON.stringify({ ...DEFAULT_KNOWLEDGE_RESEARCH_BUDGET, [field]: 1 })]);
    }
  });

  it("数据库拒绝悬空的运行、范围、轮次、来源、快照、解析块和证据关联", () => {
    const { store, reject } = fixture();
    expect(store.db.pragma("foreign_keys", { simple: true })).toBe(1);
    const references: Array<[TableKey, string[]]> = [
      ["runs", ["turn_scope_id"]], ["needs", ["run_id"]], ["rounds", ["run_id"]],
      ["receipts", ["run_id", "source_id", "content_snapshot_id", "parse_artifact_id", "block_id"]],
      ["evidence", ["run_id", "source_id", "content_snapshot_id", "parse_artifact_id", "block_id"]],
      ["relations", ["need_id", "evidence_id", "source_independence_key"]], ["actions", ["run_id", "round_id"]],
    ];
    for (const [table, columns] of references) {
      for (const column of columns) reject(table, column, ["missing-parent"], /FOREIGN KEY constraint failed/);
    }
    expect(store.db.pragma("foreign_key_check")).toEqual([]);
  });

  it("回执和证据引用的真实资料不能被删除，独立索引库身份不设跨库外键", () => {
    const { store, accept, scope, source, artifact, block } = fixture();
    const parents = [
      ["knowledge_turn_scopes", scope.id], ["sources", source.source.id],
      ["content_snapshots", source.snapshot.id], ["parse_artifacts", artifact.id],
      ["knowledge_blocks", block.id], ["knowledge_research_runs", "run-1"],
      ["knowledge_evidence_needs", "need-1"], ["knowledge_evidence_items", "evidence-1"],
      ["knowledge_research_rounds", "round-1"],
    ];
    for (const [table, id] of parents) {
      expect(() => store.db.prepare(`DELETE FROM ${table} WHERE id = ?`).run(id), table).toThrow(/FOREIGN KEY constraint failed/);
    }
    for (const table of ["receipts", "evidence"] as const) {
      const foreignKeys = store.db.pragma(`foreign_key_list(${TABLES[table]})`) as Array<{ from: string }>;
      expect(foreignKeys.map(key => key.from)).not.toContain("chunk_index_variant_id");
      expect(foreignKeys.map(key => key.from)).not.toContain("chunk_id");
      accept(table, "chunk_index_variant_id", [null, "index-database-variant"]);
      accept(table, "chunk_id", [null, "index-database-chunk"]);
    }
  });

  it("每张表的主键拒绝重复，需求和证据关联按三元组唯一", () => {
    const { store, rows } = fixture();
    for (const table of Object.keys(TABLES) as TableKey[]) {
      expect(() => insert(store, table, rows[table]), TABLES[table]).toThrow(/UNIQUE constraint failed/);
    }
    insert(store, "relations", { ...rows.relations, relation: "contradicts" });
    insert(store, "relations", { ...rows.relations, relation: "context" });
    expect(store.db.prepare(`SELECT COUNT(*) AS count FROM ${TABLES.relations}`).get().count).toBe(3);
    expect(() => insert(store, "relations", { ...rows.relations, relation: "context" })).toThrow(/UNIQUE constraint failed/);
  });

  it("同一运行的需求、轮次和动作顺序不可重复，另一运行可以独立使用相同顺序", () => {
    const { store, rows } = fixture();
    insert(store, "runs", { ...rows.runs, id: "run-2" });
    for (const table of ["needs", "rounds", "actions"] as const) {
      const indexes = store.db.pragma(`index_list(${TABLES[table]})`) as Array<{ name: string; unique: number }>;
      expect(indexes.filter(index => index.unique).map(index => {
        return store.db.pragma(`index_info(${index.name})`).map((column: { name: string }) => column.name);
      })).toContainEqual(["run_id", "ordinal"]);
      expect(() => insert(store, table, { ...rows[table], id: `${table}-duplicate` })).toThrow(/UNIQUE constraint failed/);
      insert(store, table, { ...rows[table], id: `${table}-ordinal-1`, ordinal: 1 });
      insert(store, table, {
        ...rows[table], id: `${table}-other-run`, run_id: "run-2",
        ...(table === "actions" ? { round_id: null } : {}),
      });
    }
  });

  it("证据按运行和原文位置去重，回执允许重新读取同一位置", () => {
    const { store, rows } = fixture();
    expect(() => insert(store, "evidence", { ...rows.evidence, id: "duplicate-span" })).toThrow(/UNIQUE constraint failed/);
    insert(store, "evidence", { ...rows.evidence, id: "another-span", start_offset: 1 });
    insert(store, "evidence", { ...rows.evidence, id: "another-end", end_offset: TEXT.length - 1 });
    const indexes = store.db.pragma(`index_list(${TABLES.evidence})`) as Array<{ name: string; unique: number }>;
    expect(indexes.filter(index => index.unique).map(index => {
      return store.db.pragma(`index_info(${index.name})`).map((column: { name: string }) => column.name);
    })).toContainEqual(["run_id", "parse_artifact_id", "block_id", "start_offset", "end_offset"]);
    insert(store, "runs", { ...rows.runs, id: "run-2" });
    insert(store, "evidence", { ...rows.evidence, id: "another-run-span", run_id: "run-2" });
    insert(store, "receipts", { ...rows.receipts, id: "reread-receipt" });
  });

  it("新运行和轮次的计数默认零，回执只保留定位与摘要且重启后七表数据完整", () => {
    const { store, dbPath, rows } = fixture();
    const run: Row = { ...rows.runs, id: "default-run" };
    for (const field of ["rounds_completed", "tool_calls_used", "search_calls", "read_calls", "grep_calls", "delegated_agents"]) delete run[field];
    insert(store, "runs", run);
    const round: Row = { ...rows.rounds, id: "default-round", run_id: "default-run" };
    delete round.new_evidence_count;
    insert(store, "rounds", round);
    const storedRun = store.db.prepare(`SELECT * FROM ${TABLES.runs} WHERE id = ?`).get("default-run");
    for (const field of ["rounds_completed", "tool_calls_used", "search_calls", "read_calls", "grep_calls", "delegated_agents"]) expect(storedRun[field]).toBe(0);
    expect(store.db.prepare(`SELECT new_evidence_count FROM ${TABLES.rounds} WHERE id = ?`).get("default-round").new_evidence_count).toBe(0);
    const before = Object.fromEntries((Object.keys(TABLES) as TableKey[]).map(table => [
      table, store.db.prepare(`SELECT * FROM ${TABLES[table]} ORDER BY ${table === "relations" ? "need_id" : "id"}`).all(),
    ]));
    const receipt = (before.receipts as Row[])[0];
    expect(receipt).not.toHaveProperty("canonical_text");
    expect(receipt).not.toHaveProperty("text");
    expect(JSON.stringify(receipt)).not.toContain(TEXT);
    stores.splice(stores.indexOf(store), 1);
    store.close();
    const reopened = openStore(dbPath);
    expect(reopened.db.pragma("user_version", { simple: true })).toBe(18);
    for (const table of Object.keys(TABLES) as TableKey[]) {
      expect(reopened.db.prepare(`SELECT * FROM ${TABLES[table]} ORDER BY ${table === "relations" ? "need_id" : "id"}`).all()).toEqual(before[table]);
    }
    expect(reopened.db.pragma("foreign_key_check")).toEqual([]);
  });
});
