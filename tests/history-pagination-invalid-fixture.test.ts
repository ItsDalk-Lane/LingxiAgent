/**
 * A03：非法夹具单独存放——验证重复 ID / 自引用 parentId / 环 / 缺父节点被现有生产校验识别，
 * 以及坏尾行（截断 JSON）的真实生产行为。合法性能样本（history-pagination-run-continuity.test.ts）
 * 不与非法结构混合。
 *
 * 现有生产校验的真实行为（本文件按实际行为断言，未改任何生产代码）：
 *  - 严格层 `lib/session-jsonl.ts`（readCurrentSessionBranch / projectCurrentSessionBranchEntries，
 *    对账 evidence 路径与分支头同步使用）：重复 ID → session_branch_duplicate_id；
 *    自引用与环 → session_branch_cycle；缺父节点 → session_branch_dangling_parent；
 *    截断 JSON → session_branch_invalid_json。全部抛 SessionBranchError。
 *  - SDK 层 `SessionManager.getBranch()`（loadSessionHistoryMessages 分支入口所用）：
 *    不做校验，自引用/环会沿 byId 无限回溯，实测抛 RangeError: Invalid array length。
 *  - 兼容层 `core/message-utils.ts loadSessionHistoryMessages`：分支读取抛错时 catch 后
 *    **静默降级**到逐行 raw-read fallback（设计如此："旧文件或损坏文件继续走兼容读取"），
 *    路由仍返回 200。重复 ID / 环 / 缺父节点在 fallback 中不会被拒绝——这正是非法夹具
 *    不能当性能样本的原因：它们测不到分支路径。
 */

import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { Hono } from "hono";
import fs from "fs";
import os from "os";
import path from "path";

const { readCurrentSessionBranch, SessionBranchError } = await import("../lib/session-jsonl.ts");
const { SessionManager } = await import("../lib/pi-sdk/index.ts");
const { SessionManifestStore } = await import("../core/session-manifest/store.ts");
const { loadSessionHistoryMessages } = await import("../core/message-utils.ts");

const SESSION_FILE_HEADER_ID = "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31";

function jsonlLine(id: string, parentId: string | null, message: unknown): string {
  return JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T10:00:00Z", message });
}

function headerLine(): string {
  return JSON.stringify({
    type: "session",
    version: 3,
    id: SESSION_FILE_HEADER_ID,
    cwd: "/tmp",
    timestamp: "2026-09-10T09:00:00Z",
  });
}

function assistantContent(i: number, withTool: boolean): unknown {
  return withTool
    ? [
        { type: "thinking", thinking: `思考片段 ${i}` },
        { type: "tool_use", id: `tu-${i}`, name: "read_file", input: { path: `f${i}` } },
      ]
    : [
        { type: "thinking", thinking: `最终整理 ${i}` },
        { type: "text", text: `这是最终报告：任务已完成（第 ${i} 次调用后）。` },
      ];
}

function writeLines(sessionPath: string, lines: string[]): void {
  fs.writeFileSync(sessionPath, lines.join("\n"), "utf8");
}

function expectBranchError(sessionPath: string, code: string): void {
  let caught: any = null;
  try {
    readCurrentSessionBranch(sessionPath);
  } catch (error) {
    caught = error;
  }
  expect(caught).toBeInstanceOf(SessionBranchError);
  expect(caught.code).toBe(code);
}

describe("非法夹具被生产严格分支校验识别（lib/session-jsonl.ts）", () => {
  let dir: string;

  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-invalid-fixture-"));
  });

  afterEach(() => {
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("重复 ID（toolResult 复用 assistant 的 id）→ session_branch_duplicate_id", () => {
    const sessionPath = path.join(dir, "duplicate-id.jsonl");
    // 合法链应为 a1 → r1(独立 id)；这里 r1 复用 a1 的 id，且不构成环（单独隔离这一类）。
    writeLines(sessionPath, [
      headerLine(),
      jsonlLine("u1", null, { role: "user", content: "请完成任务" }),
      jsonlLine("a1", "u1", { role: "assistant", content: assistantContent(1, true) }),
      jsonlLine("a1", "u1", {
        role: "toolResult",
        toolCallId: "tu-1",
        toolName: "read_file",
        content: "文件内容 1",
      }),
      jsonlLine("a2", "a1", { role: "assistant", content: assistantContent(2, false) }),
    ]);
    expectBranchError(sessionPath, "session_branch_duplicate_id");
  });

  it("自引用 parentId（entry.parentId === 自身 id）→ session_branch_cycle", () => {
    const sessionPath = path.join(dir, "self-parent.jsonl");
    writeLines(sessionPath, [
      headerLine(),
      jsonlLine("u1", null, { role: "user", content: "请完成任务" }),
      jsonlLine("a1", "u1", { role: "assistant", content: assistantContent(1, true) }),
      jsonlLine("r1", "r1", {
        role: "toolResult",
        toolCallId: "tu-1",
        toolName: "read_file",
        content: "文件内容 1",
      }),
    ]);
    expectBranchError(sessionPath, "session_branch_cycle");
  });

  it("三条记录互指成环（r1→r3→r2→r1）→ session_branch_cycle", () => {
    const sessionPath = path.join(dir, "cycle.jsonl");
    const toolResult = (i: number) => ({
      role: "toolResult",
      toolCallId: `tu-${i}`,
      toolName: "read_file",
      content: `文件内容 ${i}`,
    });
    writeLines(sessionPath, [
      headerLine(),
      jsonlLine("r1", "r3", toolResult(1)),
      jsonlLine("r2", "r1", toolResult(2)),
      jsonlLine("r3", "r2", toolResult(3)),
    ]);
    expectBranchError(sessionPath, "session_branch_cycle");
  });

  it("缺父节点（parentId 指向不存在的记录）→ session_branch_dangling_parent", () => {
    const sessionPath = path.join(dir, "dangling-parent.jsonl");
    writeLines(sessionPath, [
      headerLine(),
      jsonlLine("u1", null, { role: "user", content: "请完成任务" }),
      jsonlLine("a1", "ghost-missing-parent", { role: "assistant", content: assistantContent(1, false) }),
    ]);
    expectBranchError(sessionPath, "session_branch_dangling_parent");
  });

  it("SRC01 原始形状（toolResult 重复 assistant id 且 parentId 自引用）→ 严格层拒绝为 duplicate_id（重复检查先于环）", () => {
    const sessionPath = path.join(dir, "src01-shape.jsonl");
    // 修复前 writeLongRunSession 的实际形状：jsonlLine(parent, parent, toolResult)。
    const lines = [
      headerLine(),
      jsonlLine("u1", null, { role: "user", content: "请完成长任务并给出最终报告" }),
    ];
    let parent = "u1";
    for (let i = 1; i <= 6; i += 1) {
      const isFinal = i === 6;
      lines.push(jsonlLine(`a${i}`, parent, { role: "assistant", content: assistantContent(i, !isFinal) }));
      parent = `a${i}`;
      if (!isFinal) {
        lines.push(
          jsonlLine(parent, parent, {
            role: "toolResult",
            toolCallId: `tu-${i}`,
            toolName: "read_file",
            content: `文件内容 ${i}`,
          }),
        );
      }
    }
    writeLines(sessionPath, lines);
    expectBranchError(sessionPath, "session_branch_duplicate_id");
  });

  it("SDK 分支读取层（SessionManager.getBranch）对自引用/环不校验，实测以 RangeError 崩溃——即修复前性能样本静默落入兼容 fallback 的原因", () => {
    const sessionPath = path.join(dir, "self-parent.jsonl");
    writeLines(sessionPath, [
      headerLine(),
      jsonlLine("u1", null, { role: "user", content: "请完成任务" }),
      jsonlLine("a1", "u1", { role: "assistant", content: assistantContent(1, true) }),
      jsonlLine("r1", "r1", {
        role: "toolResult",
        toolCallId: "tu-1",
        toolName: "read_file",
        content: "文件内容 1",
      }),
    ]);
    let thrown: any = null;
    try {
      SessionManager.open(sessionPath, dir).getBranch();
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(RangeError);
  });
});

describe("非法/损坏夹具经真实生产读取入口的行为（按实际行为记录，不改生产代码）", () => {
  let agentsDir: string;
  let store: any;
  let tracker: { openCalls: number; openThrows: number; getBranchCalls: number; getBranchThrows: number };

  function makeEngine(sessionPath: string, manifest: any) {
    return {
      agentsDir,
      currentSessionPath: null,
      isSessionStreaming: () => false,
      agentIdFromSessionPath: () => "hana",
      getAgent: () => ({ agentName: "Hana" }),
      getSessionWorkspaceMount: () => null,
      getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
      getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
      openSessionManagerAtCurrentBranch: (p: string, dir: string) => {
        tracker.openCalls += 1;
        let manager: any;
        try {
          manager = SessionManager.open(p, dir);
        } catch (error) {
          tracker.openThrows += 1;
          throw error;
        }
        const rawGetBranch = manager.getBranch.bind(manager);
        manager.getBranch = (...args: any[]) => {
          tracker.getBranchCalls += 1;
          try {
            return rawGetBranch(...args);
          } catch (error) {
            tracker.getBranchThrows += 1;
            throw error;
          }
        };
        return manager;
      },
    };
  }

  beforeEach(() => {
    agentsDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-invalid-route-"));
    store = new SessionManifestStore({ dbPath: path.join(agentsDir, "session-manifest.db") });
    tracker = { openCalls: 0, openThrows: 0, getBranchCalls: 0, getBranchThrows: 0 };
  });

  afterEach(() => {
    store?.close();
    fs.rmSync(agentsDir, { recursive: true, force: true });
  });

  it("SRC01 形状经 loadSessionHistoryMessages：分支读取抛错，兼容 raw-read fallback 静默接管（路由仍 200，如实记录兼容行为）", async () => {
    const sessionDir = path.join(agentsDir, "hana", "sessions");
    const sessionPath = path.join(sessionDir, "src01-shape.jsonl");
    fs.mkdirSync(sessionDir, { recursive: true });
    const lines = [
      headerLine(),
      jsonlLine("u1", null, { role: "user", content: "请完成长任务并给出最终报告" }),
    ];
    let parent = "u1";
    for (let i = 1; i <= 6; i += 1) {
      const isFinal = i === 6;
      lines.push(jsonlLine(`a${i}`, parent, { role: "assistant", content: assistantContent(i, !isFinal) }));
      parent = `a${i}`;
      if (!isFinal) {
        lines.push(
          jsonlLine(parent, parent, {
            role: "toolResult",
            toolCallId: `tu-${i}`,
            toolName: "read_file",
            content: `文件内容 ${i}`,
          }),
        );
      }
    }
    writeLines(sessionPath, lines);
    const manifest = store.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
    const engine = makeEngine(sessionPath, manifest);

    const messages = await loadSessionHistoryMessages(engine, sessionPath);

    // 兼容行为（如实记录）：分支入口被调用且以异常告终（SDK getBranch 对自引用/环 RangeError），
    // loadSessionHistoryMessages catch 后走 raw-read fallback——不报错、不丢事件，返回物理行投影。
    expect(tracker.openCalls).toBe(1);
    expect(tracker.getBranchCalls).toBe(1);
    expect(tracker.getBranchThrows).toBe(1);
    // fallback 投影包含全部可显示记录：1 user + 6 assistant（toolResult 不占 display，但物理保留）。
    expect(messages.filter((m: any) => m.role === "user")).toHaveLength(1);
    expect(messages.filter((m: any) => m.role === "assistant")).toHaveLength(6);
    expect(messages.filter((m: any) => m.role === "toolResult")).toHaveLength(5);

    // 经真实 Hono 路由：200 正常返回（静默降级，而非报错）。
    const { createSessionsRoute } = await import("../server/routes/sessions.ts");
    const app = new Hono();
    app.route("/api", createSessionsRoute(engine));
    const res = await app.request(
      `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}`,
    );
    expect(res.status).toBe(200);
    const data = await res.json();
    expect(data.messages).toHaveLength(7); // 1 user + 6 assistant
    expect(data.hasMore).toBe(false);
  });

  it("坏尾行（截断 JSON）：严格层拒绝为 invalid_json；生产读取先经 oversized-repair 修复落盘（产生 .repair.json 备份并剔除坏行），分支路径零异常、fallback 次数 0", async () => {
    const sessionDir = path.join(agentsDir, "hana", "sessions");
    const sessionPath = path.join(sessionDir, "bad-tail.jsonl");
    fs.mkdirSync(sessionDir, { recursive: true });
    const validLines = [
      headerLine(),
      jsonlLine("u1", null, { role: "user", content: "请完成任务" }),
      jsonlLine("a1", "u1", { role: "assistant", content: assistantContent(1, true) }),
      jsonlLine("r1", "a1", {
        role: "toolResult",
        toolCallId: "tu-1",
        toolName: "read_file",
        content: "文件内容 1",
      }),
      jsonlLine("a2", "r1", { role: "assistant", content: assistantContent(2, false) }),
    ];
    // 单独构造坏尾行，不混入合法性能样本：截断的 JSON 物理行。
    writeLines(sessionPath, [...validLines, '{"type":"message","id":"tail-broken","parentId":"a2","mess']);

    // 严格层（不修复文件的对账读取器）拒绝：session_branch_invalid_json。
    const pristineCopy = path.join(agentsDir, "bad-tail-pristine-copy.jsonl");
    fs.copyFileSync(sessionPath, pristineCopy);
    expectBranchError(pristineCopy, "session_branch_invalid_json");

    const manifest = store.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
    const engine = makeEngine(sessionPath, manifest);
    const messages = await loadSessionHistoryMessages(engine, sessionPath);

    // 真实行为：repairOversizedSessionEntriesInFile 先剔除坏行并落盘（带备份），
    // 随后 SessionManager 分支读取零异常 → 走的是分支路径而非 fallback。
    expect(tracker.openCalls).toBe(1);
    expect(tracker.getBranchCalls).toBe(1);
    expect(tracker.getBranchThrows).toBe(0);
    expect(tracker.openThrows).toBe(0);
    expect(fs.existsSync(`${sessionPath}.repair.json`)).toBe(true);
    for (const line of fs.readFileSync(sessionPath, "utf8").split("\n")) {
      if (!line.trim()) continue;
      expect(() => JSON.parse(line)).not.toThrow();
    }
    expect(fs.readFileSync(sessionPath, "utf8")).not.toContain("tail-broken");
    // 修复后合法记录完整保留：1 user + 2 assistant + 1 toolResult，分支链完好。
    expect(messages.filter((m: any) => m.role === "user")).toHaveLength(1);
    expect(messages.filter((m: any) => m.role === "assistant")).toHaveLength(2);
    expect(messages.filter((m: any) => m.role === "toolResult")).toHaveLength(1);
    const projection = readCurrentSessionBranch(sessionPath);
    expect(projection.lineage.map((entry: any) => entry.id)).toEqual(["u1", "a1", "r1", "a2"]);
    expect(projection.selectedLeafId).toBe("a2");
  });
});
