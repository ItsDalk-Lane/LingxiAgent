/**
 * B03 目录构建测试（X01 / X08 / X09 / X13 + I04 性质测试）：
 *  - 重复 id / 自环 / 双向环 / 缺父 / 混合 id / head 指向缺失条目 → 拒绝码与严格层
 *    （projectCurrentSessionBranchEntries）逐字一致，目录一律不建立；
 *  - head 行缺失 vs leafId=null vs append_recovery vs observedTail 记录弃分支 vs
 *    continuesDiscardedObservedTail：真实 SessionManifestStore（SQLite），目录的
 *    persistBranchHead 写回与旧路径 readManifestSessionBranch(persistRecovery=true)
 *    的 head 行终态逐字段一致（leafId / observedTailLeafId / reason）；
 *  - todos 指针 = 最后合法快照、坏快照跳过、空快照清空，派生 todos 与
 *    extractLatestTodos 逐字段一致；
 *  - 性质测试：任意合法夹具 build 出的 ProjectionContext 与
 *    buildProjectionContextFromMessages 全字段等价（I04）；
 *  - measure() 保守估算与 budget_exceeded。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { scanHistoryFile } from "../server/history-read/scan.ts";
import {
  buildHistoryDirectory,
  measureDirectoryBytes,
  MAX_SINGLE_DIRECTORY_BYTES,
} from "../server/history-read/directory.ts";
import { buildProjectionContextFromMessages } from "../server/history-read/projection-context.ts";
import type { HistoryReadContext } from "../server/history-read/types.ts";
import {
  projectCurrentSessionBranchEntries,
  SessionBranchError,
} from "../lib/session-jsonl.ts";
import { readManifestSessionBranch } from "../core/session-branch-head.ts";
import { projectBranchHistory } from "../core/message-utils.ts";
import { extractLatestTodos } from "../lib/tools/todo-compat.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import {
  MESSAGE_ORIGIN_RECORD_TYPE,
  MESSAGE_PRESENTATION_RECORD_TYPE,
  AGENT_REVIEW_RECORD_TYPE,
} from "../core/desktop-session-submit.ts";
import { MODEL_CALL_REFERENCE_RECORD_TYPE } from "../lib/llm/model-call-correlation.ts";
import { TURN_INPUT_CONSUMPTION_EVENT_TYPE } from "../lib/turn-input-presentation.ts";
import { SESSION_COLLAB_DECISION_RECORD_TYPE } from "../lib/session-collab/decision-record.ts";
import { DEFERRED_RESULT_MESSAGE_TYPE } from "../lib/deferred-result-notification.ts";
import { TODO_STATE_CUSTOM_TYPE } from "../lib/tools/todo-constants.ts";
import { buildLongRunFixtureLines } from "../scripts/lib/history-read-fixture.mjs";

const tmpDirs: string[] = [];

afterEach(() => {
  while (tmpDirs.length) {
    const dir = tmpDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function makeTmpDir(prefix = "hana-build-test-"): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tmpDirs.push(dir);
  return dir;
}

function jsonlLine(id: string, parentId: string | null, message: any) {
  return JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T10:00:00Z", message });
}

const HEADER = JSON.stringify({ type: "session", version: 3, id: "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" });

function userLine(id: string, parentId: string | null, content: string) {
  return jsonlLine(id, parentId, { role: "user", content });
}
function assistantLine(id: string, parentId: string | null, content = "回复") {
  return jsonlLine(id, parentId, { role: "assistant", content });
}
function customLine(id: string, parentId: string | null, customType: string, data: any) {
  return JSON.stringify({ type: "custom", id, parentId, timestamp: "2026-09-10T10:00:00Z", customType, data });
}
function customMessageLine(id: string, parentId: string | null, customType: string, extra: Record<string, any>) {
  return JSON.stringify({ type: "custom_message", id, parentId, timestamp: "2026-09-10T10:00:00Z", customType, content: "", display: false, ...extra });
}

function writeFixture(name: string, lines: string[]): string {
  const p = path.join(makeTmpDir(), name);
  fs.writeFileSync(p, [...lines].join("\n") + "\n");
  return p;
}

async function scanPath(p: string, opts: Record<string, any> = {}) {
  return await scanHistoryFile(p, { capturedLength: fs.statSync(p).size, ...opts });
}

function makeCtx(p: string, opts: {
  sessionId?: string | null;
  store?: SessionManifestStore | null;
  branchHeadRowExists?: boolean;
  branchHeadRow?: any;
  locatorPath?: string | null;
  publicRevision?: string | null;
} = {}): HistoryReadContext {
  const stat = fs.statSync(p);
  const sessionId = opts.sessionId ?? null;
  return {
    sessionPath: p,
    sessionId,
    locatorPath: opts.locatorPath === undefined ? p : opts.locatorPath,
    publicRevision: opts.publicRevision === undefined ? `${stat.size}:${stat.mtimeMs}` : opts.publicRevision,
    fileIdentity: { size: stat.size, mtimeMs: stat.mtimeMs },
    branchHeadRowExists: opts.branchHeadRowExists ?? false,
    branchHeadRow: opts.branchHeadRow ?? null,
    ...(opts.store && sessionId
      ? { persistBranchHead: (state: any) => opts.store!.setBranchHead(sessionId, state) }
      : {}),
  };
}

function deepEqual(a: any, b: any): boolean {
  if (a === b) return true;
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return a === b;
  if (a instanceof Map !== b instanceof Map) return false;
  if (a instanceof Set !== b instanceof Set) return false;
  if (a instanceof Map) {
    if (a.size !== b.size) return false;
    for (const [k, v] of a) {
      if (!b.has(k) || !deepEqual(v, b.get(k))) return false;
    }
    return true;
  }
  if (a instanceof Set) {
    if (a.size !== b.size) return false;
    for (const v of a) if (!b.has(v)) return false;
    return true;
  }
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) {
    return a.length === b.length && a.every((v, i) => deepEqual(v, b[i]));
  }
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  return ka.every((k) => k in b && deepEqual(a[k], b[k]));
}

describe("目录构建拒绝码与严格层一致（X01）", () => {
  const cases: Array<{ name: string; lines: string[]; expectedCode: string }> = [
    {
      name: "重复 id",
      lines: [HEADER, userLine("dup", null, "一"), userLine("dup", "dup", "二")],
      expectedCode: "session_branch_duplicate_id",
    },
    {
      name: "自环（parentId 指向自身）",
      lines: [HEADER, jsonlLine("x", "x", { role: "user", content: "自环" })],
      expectedCode: "session_branch_cycle",
    },
    {
      name: "双向环",
      lines: [HEADER, jsonlLine("c1", "c2", { role: "user", content: "环" }), jsonlLine("c2", "c1", { role: "assistant", content: "环" })],
      expectedCode: "session_branch_cycle",
    },
    {
      name: "缺父节点",
      lines: [HEADER, userLine("u1", null, "一"), userLine("u2", "ghost", "二")],
      expectedCode: "session_branch_dangling_parent",
    },
    {
      name: "混合 id 与无 id 条目",
      lines: [HEADER, JSON.stringify({ type: "message", message: { role: "user", content: "无 id" } }), userLine("u2", null, "有 id")],
      expectedCode: "session_branch_invalid_id",
    },
  ];

  for (const tc of cases) {
    it(tc.name, async () => {
      const p = writeFixture("reject.jsonl", tc.lines);
      // 严格层拒绝码（直接调用现产实现）
      let strictCode = "";
      try {
        projectCurrentSessionBranchEntries(JSON.parse(`[${tc.lines.join(",")}]`), { branchHead: null, filePath: p });
      } catch (err) {
        expect(err).toBeInstanceOf(SessionBranchError);
        strictCode = (err as SessionBranchError).code;
      }
      expect(strictCode).toBe(tc.expectedCode);

      const scan = await scanPath(p);
      const result = await buildHistoryDirectory(scan, makeCtx(p));
      expect(result.directory).toBeNull();
      expect((result as any).reason).toBe("directory_invalid");
      expect((result as any).detail).toBe(tc.expectedCode);
    });
  }

  it("head 指向缺失条目 → session_branch_head_missing", async () => {
    const p = writeFixture("head-missing.jsonl", [HEADER, userLine("u1", null, "一")]);
    const scan = await scanPath(p);
    const result = await buildHistoryDirectory(scan, makeCtx(p, {
      branchHeadRowExists: true,
      branchHeadRow: { leafId: "ghost", observedTailLeafId: "ghost" },
    }));
    expect(result.directory).toBeNull();
    expect((result as any).reason).toBe("directory_invalid");
    expect((result as any).detail).toBe("session_branch_head_missing");
  });

  it("revision 未知 → revision_unknown（I08）", async () => {
    const p = writeFixture("rev-null.jsonl", [HEADER, userLine("u1", null, "一")]);
    const scan = await scanPath(p);
    const result = await buildHistoryDirectory(scan, makeCtx(p, { publicRevision: null }));
    expect(result).toEqual({ directory: null, reason: "revision_unknown" });
  });

  it("已完成位置损坏（corrupt_record）→ directory_invalid", async () => {
    const p = writeFixture("scan-corrupt.jsonl", [HEADER, "{broken"]);
    const scan = await scanPath(p);
    const result = await buildHistoryDirectory(scan, makeCtx(p));
    expect(result.directory).toBeNull();
    expect((result as any).reason).toBe("directory_invalid");
    expect((result as any).detail).toBe("corrupt_record");
  });
});

describe("head 状态与恢复写回（X08/X09，真实 SessionManifestStore）", () => {
  function chainLines(ids: string[]): string[] {
    return ids.map((id, i) => (i === 0 ? userLine(id, null, "开头") : assistantLine(id, ids[i - 1])));
  }

  /** 同一夹具 + 同一 seed head：旧路径 readManifestSessionBranch 与新路径目录写回，head 行终态逐字段一致。 */
  async function runScenario(scenario: {
    name: string;
    base: string[];
    appended?: string[];
    seedHead?: { leafId: string | null; observedTailLeafId: string | null } | null;
  }) {
    const rows: Array<{ side: string; row: any; branch?: any }> = [];
    for (const side of ["old", "new"] as const) {
      const dir = makeTmpDir(`hana-head-${scenario.name}-${side}-`);
      const p = path.join(dir, "s.jsonl");
      fs.writeFileSync(p, [...scenario.base, ...(scenario.appended ?? [])].join("\n") + "\n");
      const store = new SessionManifestStore({ dbPath: path.join(dir, "manifest.db") });
      try {
        const manifest = store.createForPath({ sessionPath: p, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
        if (scenario.seedHead) {
          store.setBranchHead(manifest.sessionId, { ...scenario.seedHead, reason: "test_seed" });
        }
        const seededRow = store.getBranchHead(manifest.sessionId);
        if (side === "old") {
          readManifestSessionBranch({ store, sessionId: manifest.sessionId, sessionPath: p, persistRecovery: true });
          rows.push({ side, row: store.getBranchHead(manifest.sessionId) });
        } else {
          const scan = await scanPath(p);
          const result = await buildHistoryDirectory(scan, makeCtx(p, {
            sessionId: manifest.sessionId,
            store,
            branchHeadRowExists: seededRow != null,
            branchHeadRow: seededRow,
          }));
          expect(result.directory).not.toBeNull();
          rows.push({ side, row: store.getBranchHead(manifest.sessionId), branch: (result.directory as any).branch });
        }
      } finally {
        store.close();
      }
    }
    const [oldSide, newSide] = rows;
    // 终态与写回路径可控字段逐字段一致；sessionId/updatedAt 由两个独立 store 各自生成，不可比
    const stripStoreManaged = (row: any) => (row == null
      ? row
      : (({ updatedAt, sessionId, ...rest }: any) => rest)(row));
    expect(stripStoreManaged(newSide.row)).toEqual(stripStoreManaged(oldSide.row));
    return { oldRow: oldSide.row, newRow: newSide.row, branch: newSide.branch };
  }

  it("head 行缺失 → legacy_backfill（双方写 leafId=物理尾）", async () => {
    const ids = ["u1", "a1", "a2", "a3"];
    const { oldRow, newRow, branch } = await runScenario({ name: "no-row", base: chainLines(ids), seedHead: null });
    const tail = "a3";
    expect(oldRow).toMatchObject({ leafId: tail, observedTailLeafId: tail, reason: "branch_read_legacy_backfill" });
    expect(branch.headRowExists).toBe(false);
    expect(branch.headResolution).toBe("legacy_tail");
  });

  it("leafId=null 且 observedTail 未变 → 显式空选择保持，不写回（persisted_head）", async () => {
    const ids = ["u1", "a1"];
    const tail = "a1";
    const seed = { leafId: null, observedTailLeafId: tail };
    const { oldRow, newRow, branch } = await runScenario({ name: "null-leaf", base: chainLines(ids), seedHead: seed });
    expect(newRow.leafId).toBeNull();
    expect(newRow.observedTailLeafId).toBe(tail);
    expect(newRow.reason).toBe("test_seed"); // 未写回，seed reason 原样保留
    expect(branch.headResolution).toBe("persisted_head");
    expect(branch.selectedLeafId).toBeNull();
  });

  it("合法追加 → append_recovery（双方同 reason 同 leaf）", async () => {
    const base = chainLines(["u1", "a1", "a2", "a3", "a4", "a5"]);
    const appended = ["a6", "a7", "a8", "a9", "a10"].map((id, i, arr) => assistantLine(id, i === 0 ? "a5" : arr[i - 1]));
    const { oldRow, newRow, branch } = await runScenario({
      name: "append",
      base,
      appended,
      seedHead: { leafId: "a5", observedTailLeafId: "a5" },
    });
    expect(oldRow).toMatchObject({ leafId: "a10", observedTailLeafId: "a10", reason: "append_recovery" });
    expect(branch.headResolution).toBe("append_recovery");
  });

  it("observedTail 记录弃分支：选择保持、observedTail 推进（branch_read_observe_tail）", async () => {
    // 物理序：u1→a1→a2→a3 主链 + 弃分支 b1(a1)→b2(b1)；物理尾 = b2
    const base = [
      ...chainLines(["u1", "a1", "a2", "a3"]),
      assistantLine("b1", "a1"),
      assistantLine("b2", "b1"),
    ];
    const { oldRow, newRow, branch } = await runScenario({
      name: "discarded",
      base,
      seedHead: { leafId: "a3", observedTailLeafId: "a3" },
    });
    expect(oldRow).toMatchObject({ leafId: "a3", observedTailLeafId: "b2", reason: "branch_read_observe_tail" });
    expect(branch.headResolution).toBe("persisted_head");
    expect(branch.selectedLeafId).toBe("a3");
    expect(branch.physicalTailLeafId).toBe("b2");
  });

  it("continuesDiscardedObservedTail：物理尾是弃分支 observedTail 的后代 → 不追加恢复，保持显式选择", async () => {
    const base = [
      ...chainLines(["u1", "a1", "a2", "a3"]),
      assistantLine("b1", "a1"),
      assistantLine("b2", "b1"),
    ];
    const { oldRow, newRow, branch } = await runScenario({
      name: "continues-discarded",
      base,
      seedHead: { leafId: "a1", observedTailLeafId: "b1" },
    });
    expect(oldRow).toMatchObject({ leafId: "a1", observedTailLeafId: "b2", reason: "branch_read_observe_tail" });
    expect(branch.headResolution).toBe("persisted_head");
    expect(branch.selectedLeafId).toBe("a1");
  });
});

describe("todos 指针（X13）", () => {
  const legacyTodos = { todos: [{ id: "1", text: "旧格式", done: false }] };
  const newTodos = { todos: [{ content: "任务", activeForm: "任务", status: "in_progress" }] };

  it("最后合法快照 = 指针；派生 todos 与 extractLatestTodos 一致", async () => {
    const lines = [
      HEADER,
      userLine("u1", null, "开始"),
      jsonlLine("r1", "u1", { role: "toolResult", toolCallId: "tu-1", toolName: "todo", content: "ok", details: legacyTodos }),
      assistantLine("a1", "r1"),
      customMessageLine("c1", "a1", TODO_STATE_CUSTOM_TYPE, { details: { ...newTodos, removed: false } }),
      assistantLine("a2", "c1"),
    ];
    const p = writeFixture("todos.jsonl", lines);
    const scan = await scanPath(p);
    const result = await buildHistoryDirectory(scan, makeCtx(p));
    expect(result.directory).not.toBeNull();
    const { directory, context } = result as any;
    const c1Index = context.recordFacts.findIndex((f: any) => f.entryId === "c1");
    expect(c1Index).toBeGreaterThan(0);
    expect(context.todoSnapshotSourceIndex).toBe(c1Index);
    expect(directory.assoc.todoSnapshot).toEqual({ sourceIndex: c1Index });
    // 派生 todos 与 extractLatestTodos 逐字段一致
    const projection = projectCurrentSessionBranchEntries(scan.entries, { branchHead: null, filePath: p });
    const byId = new Map(scan.entries.filter((e: any) => e?.id != null).map((e: any) => [e.id, e]));
    const sourceMessages = projectBranchHistory(projection.lineage.map((le: any) => byId.get(le.id)), "");
    const expectedTodos = extractLatestTodos(sourceMessages);
    expect(expectedTodos).toEqual([{ content: "任务", activeForm: "任务", status: "in_progress" }]);
    expect(context.todoSnapshot.removed).toBe(false);
    expect(context.todoSnapshot.todos).toEqual(expectedTodos);
    expect(directory.session.displayTotal).toBeGreaterThan(0);
  });

  it("坏快照跳过：指针指向此前最后一条合法快照", async () => {
    const lines = [
      HEADER,
      userLine("u1", null, "开始"),
      jsonlLine("r1", "u1", { role: "toolResult", toolCallId: "tu-1", toolName: "todo", content: "ok", details: legacyTodos }),
      assistantLine("a1", "r1"),
      customMessageLine("c1", "a1", TODO_STATE_CUSTOM_TYPE, { details: newTodos }),
      jsonlLine("r2", "c1", { role: "toolResult", toolCallId: "tu-2", toolName: "todo", content: "坏快照", details: {} }),
    ];
    const p = writeFixture("todos-bad.jsonl", lines);
    const scan = await scanPath(p);
    const result = await buildHistoryDirectory(scan, makeCtx(p));
    const { directory, context } = result as any;
    const c1Index = context.recordFacts.findIndex((f: any) => f.entryId === "c1");
    expect(context.todoSnapshotSourceIndex).toBe(c1Index);
    expect(directory.assoc.todoSnapshot).toEqual({ sourceIndex: c1Index });
  });

  it("空快照清空：指针指向清空记录，todos 为 []", async () => {
    const lines = [
      HEADER,
      userLine("u1", null, "开始"),
      jsonlLine("r1", "u1", { role: "toolResult", toolCallId: "tu-1", toolName: "todo", content: "ok", details: legacyTodos }),
      assistantLine("a1", "r1"),
      customMessageLine("c2", "a1", TODO_STATE_CUSTOM_TYPE, { details: { todos: [], removed: true } }),
    ];
    const p = writeFixture("todos-cleared.jsonl", lines);
    const scan = await scanPath(p);
    const result = await buildHistoryDirectory(scan, makeCtx(p));
    const { directory, context } = result as any;
    const c2Index = context.recordFacts.findIndex((f: any) => f.entryId === "c2");
    expect(context.todoSnapshotSourceIndex).toBe(c2Index);
    expect(directory.assoc.todoSnapshot).toEqual({ sourceIndex: c2Index });
    const snapshot = context.todoSnapshot;
    expect(snapshot.removed).toBe(true);
    // 与 extractLatestTodos 输出契约一致：removed → []
    expect(snapshot.removed ? [] : snapshot.todos).toEqual([]);
  });
});

describe("I04 性质测试：build 产出的 ProjectionContext 与 buildProjectionContextFromMessages 全字段等价", () => {
  async function expectContextEquivalence(name: string, lines: string[]) {
    const p = writeFixture(`${name}.jsonl`, lines);
    const scan = await scanPath(p);
    const result = await buildHistoryDirectory(scan, makeCtx(p));
    expect(result.directory).not.toBeNull();
    const { directory, context } = result as any;

    // 独立复算（与 build 相同的生产管线，测试内重新走一遍）
    const projection = projectCurrentSessionBranchEntries(scan.entries, { branchHead: null, filePath: p });
    const byId = new Map(scan.entries.filter((e: any) => e?.id != null).map((e: any) => [e.id, e]));
    const branchEntries = projection.lineage.map((le: any) => byId.get(le.id));
    const correlationSessionId = null; // ctx.sessionId=null 且 locator 绑定 → 与 build 内部一致
    const sourceMessages = projectBranchHistory(branchEntries, correlationSessionId);
    const expected = buildProjectionContextFromMessages(sourceMessages);

    expect(deepEqual(context, expected)).toBe(true);

    // 目录事实与上下文的最小一致性
    expect(directory.records.length).toBe(sourceMessages.length);
    expect(directory.records.map((r: any) => r.sourceIndex)).toEqual(context.recordFacts.map((f: any) => f.sourceIndex));
    expect(directory.session.displayTotal).toBe(expected.displayTotal);
    expect([...directory.displayableSourceIndexes]).toEqual(
      [...directory.displayableSourceIndexes].sort((a: number, b: number) => a - b),
    );
    return { directory, context, sourceMessages };
  }

  it("长 Run 合法夹具（n=30，工具结果链）", async () => {
    const { directory } = (await expectContextEquivalence("longrun", buildLongRunFixtureLines(30))) as any;
    expect(directory.records.length).toBe(2 * 30);
    expect(directory.assoc.toolResultByCallId.size).toBe(29);
    expect(directory.displayableSourceIndexes.length).toBe(31);
    expect(directory.session.displayTotal).toBe(31);
    expect(directory.branch.lineageEntryIds.length).toBe(60);
    expect(directory.file.physicalCount).toBe(61);
  });

  it("混合夹具（origin/presentation/review/modelCall/deferred/turn-input/collab/todo）", async () => {
    const lines = [
      HEADER,
      customLine("om", null, MESSAGE_ORIGIN_RECORD_TYPE, { origin: "rc", displayText: "来自 RC" }),
      userLine("u1", "om", "开始"),
      assistantLine("a1", "u1"),
      customLine("pm", "a1", MESSAGE_PRESENTATION_RECORD_TYPE, { displayText: "展示文本", skills: [] }),
      customLine("rv", "pm", AGENT_REVIEW_RECORD_TYPE, { status: "completed", displayText: "评审" }),
      userLine("u2", "rv", "继续"),
      assistantLine("a2", "u2"),
      customLine("mc", "a2", MODEL_CALL_REFERENCE_RECORD_TYPE, { schemaVersion: 1, modelCallId: "mc-1", traceId: "t-1", parentCallId: null }),
      assistantLine("a3", "mc"),
      customMessageLine("dr", "a3", DEFERRED_RESULT_MESSAGE_TYPE, { content: JSON.stringify({ type: "image-generation", taskId: "task-1", status: "success", result: "图片" }) }),
      customLine("ti", "dr", TURN_INPUT_CONSUMPTION_EVENT_TYPE, { deliveryId: "d-1", input: { entryId: "u1" }, assistant: { entryId: "a1" }, block: { type: "interlude" } }),
      customLine("cd", "ti", SESSION_COLLAB_DECISION_RECORD_TYPE, { suggestionId: "s-1", status: "confirmed", resultSessionId: "sess-x" }),
      customMessageLine("tc", "cd", TODO_STATE_CUSTOM_TYPE, { details: { todos: [{ content: "任务", activeForm: "任务", status: "completed" }] } }),
      assistantLine("a4", "tc"),
    ];
    const { directory, context, sourceMessages } = (await expectContextEquivalence("mixed", lines)) as any;

    // 目录指针表与上下文注释一致（指针指向 custom 记录自身）
    // sourceMessages：om=0, u1=1, a1=2, pm=3, rv=4, u2=5, a2=6, mc=7, a3=8, dr=9, ...
    const u1Index = context.recordFacts.findIndex((f: any) => f.entryId === "u1");
    const u2Index = context.recordFacts.findIndex((f: any) => f.entryId === "u2");
    const omIndex = context.recordFacts.findIndex((f: any) => f.entryId === "om");
    const pmIndex = context.recordFacts.findIndex((f: any) => f.entryId === "pm");
    const rvIndex = context.recordFacts.findIndex((f: any) => f.entryId === "rv");
    const a3Index = context.recordFacts.findIndex((f: any) => f.entryId === "a3");
    expect(u1Index).toBe(1);
    expect(u2Index).toBe(5);
    expect(directory.assoc.originBySourceIndex.get(u1Index)).toEqual({ originRecordSourceIndex: omIndex });
    expect(directory.assoc.presentationBySourceIndex.get(u2Index)).toEqual({ presentationRecordSourceIndex: pmIndex });
    expect(directory.assoc.agentReviewBySourceIndex.get(u2Index)).toEqual({ reviewRecordSourceIndex: rvIndex, completed: true });

    // Run 边界与 consumption/collab/modelCall 事实进入目录
    expect(directory.assoc.turnInputByAssistantEntryId.get("a1")).toBe("u1");
    expect(JSON.parse(JSON.stringify([...directory.assoc.consumptionDeliveryIds]))).toEqual(["d-1"]);
    expect([...directory.assoc.consumptionEntryIds]).toEqual(["u1"]);
    expect(directory.assoc.collabDecisionBySuggestionId.get("s-1")).toEqual({ status: "confirmed", resultSessionId: "sess-x" });
    expect(directory.assoc.modelCallRefBySourceIndex.get(a3Index))
      .toEqual({ modelCallId: "mc-1", traceId: "t-1", parentCallId: null });
    expect(directory.assoc.todoSnapshot).toEqual({ sourceIndex: context.todoSnapshotSourceIndex });
    expect(directory.assoc.deferredInterludeAnchors.length).toBe(1);

    // 记录级补齐字段（runOrdinal/Run 边界/timestamp）
    const a3Record = directory.records.at(a3Index)!;
    expect(a3Record.runOrdinal).toBeGreaterThan(0);
    expect(typeof a3Record.turnStartIndex).toBe("number");
    expect(typeof a3Record.turnEndIndex).toBe("number");
    const sourceA3 = sourceMessages.find((m: any) => m.id === "a3");
    expect(a3Record.timestamp).toBe(sourceA3.timestamp);
  });

  it("recordFacts before 状态与目录记录一致（易碎点 1/2/7）", async () => {
    const lines = [
      HEADER,
      userLine("u1", null, "一"),
      jsonlLine("a-hide", "u1", { role: "assistant", content: "" }), // 不可见 assistant：仍推大 ordinal
      jsonlLine("r1", "a-hide", { role: "toolResult", toolCallId: "tu-1", toolName: "read", content: "结果" }),
      assistantLine("a1", "r1"),
    ];
    const p = writeFixture("before-state.jsonl", lines);
    const scan = await scanPath(p);
    const result = await buildHistoryDirectory(scan, makeCtx(p));
    const { directory } = result as any;
    const byId = new Map<string, any>(directory.records.map((r: any) => [r.entryId, r]));
    // a-hide 不可见：displayIndex null、但 assistantOrdinal 先加后判 → a1 的 before = 1
    expect(byId.get("a-hide").visible).toBe(false);
    expect(byId.get("a-hide").displayIndex).toBeNull();
    expect(byId.get("a1").assistantOrdinalBefore).toBe(1);
    expect(byId.get("u1").displayIndex).toBe(0);
    expect(byId.get("a1").displayIndex).toBe(1);
    // toolResult 锚点 = displayIndexBefore - 1（u1 占 0 号后计数 = 1 → 锚点 0）
    expect(byId.get("r1").displayIndexBefore).toBe(1);
    const anchor = directory.blockAnchorByAfterIndex.find((a: any) => a.sourceIndex === byId.get("r1").sourceIndex);
    expect(anchor).toEqual({ afterIndex: 0, sourceIndex: byId.get("r1").sourceIndex });
  });
});

describe("measure 与预算", () => {
  it("measureDirectoryBytes：string×字节系数、Map/数组条目成本、环安全", () => {
    expect(measureDirectoryBytes(null)).toBe(0);
    expect(measureDirectoryBytes("abc")).toBe(16 + 3 * 2);
    expect(measureDirectoryBytes("中")).toBe(16 + 3 * 2);
    const small = { a: 1 };
    const big = { a: 1, b: "xy".repeat(10) };
    expect(measureDirectoryBytes(big)).toBeGreaterThan(measureDirectoryBytes(small));
    const map = new Map([["k", "v"]]);
    expect(measureDirectoryBytes(map)).toBeGreaterThan(measureDirectoryBytes(new Map()));
    const arr = Array.from({ length: 100 }, () => "x");
    expect(measureDirectoryBytes(arr)).toBeGreaterThan(measureDirectoryBytes(["x"]));
    const circular: any = { self: null };
    circular.self = circular;
    expect(measureDirectoryBytes(circular)).toBeGreaterThan(0);
  });

  it("目录超预算 → budget_exceeded，不截断历史", async () => {
    const p = writeFixture("budget.jsonl", buildLongRunFixtureLines(30));
    const scan = await scanPath(p);
    const result = await buildHistoryDirectory(scan, makeCtx(p), { maxDirectoryBytes: 64 });
    expect(result.directory).toBeNull();
    expect((result as any).reason).toBe("budget_exceeded");
  });

  it("默认预算常量为 16 MiB", () => {
    expect(MAX_SINGLE_DIRECTORY_BYTES).toBe(16 * 1024 * 1024);
  });
});
