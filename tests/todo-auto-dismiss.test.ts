/**
 * 清单改版阶段 4：新请求被正式接受后的自动收纳（A08 / A09）。
 *
 * - 已结束（全完成或完成+取消）且未收纳的清单：接受新请求后自动收纳，
 *   历史记录保留，重开会话不重新弹出。
 * - 仍有未完成项的清单：不受新请求影响，继续保留（A07）。
 * - 发送失败 / 请求未被接受时不会调用本函数（由调用点位置保证，
 *   见 server/routes/chat.ts hub.send 成功分支）。
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import fs from "fs/promises";
import path from "path";
import os from "os";
import { dismissFinishedTodosOnPromptAccepted } from "../server/routes/chat.ts";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import { loadLatestTodoSnapshotFromSessionFile } from "../lib/tools/todo-compat.ts";

let tmpDir: string;

beforeEach(async () => {
  tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "todo-auto-dismiss-"));
});

afterEach(async () => {
  await fs.rm(tmpDir, { recursive: true, force: true });
});

async function seedSession(todos: any[]) {
  const sessionDir = path.join(tmpDir, "agents", "hana", "sessions");
  const manager = SessionManager.create("/tmp/workspace", sessionDir);
  const sessionPath = manager.getSessionFile();
  manager.appendMessage({
    role: "assistant",
    content: [{ type: "text", text: "working" }],
    api: "test",
    provider: "test",
    model: "test",
    stopReason: "toolUse",
    timestamp: Date.now(),
  } as any);
  manager.appendMessage({
    role: "toolResult",
    toolCallId: "todo-seed",
    toolName: "todo_write",
    content: [{ type: "text", text: "seed" }],
    isError: false,
    timestamp: Date.now(),
    details: { todoVersion: 2, todos },
  } as any);
  const engine = {
    getSessionByPath: vi.fn(() => null),
    openSessionManagerAtCurrentBranch: vi.fn(() => manager),
    syncSessionBranchHead: vi.fn(),
    emitEvent: vi.fn(),
  };
  return { engine, manager, sessionPath };
}

describe("dismissFinishedTodosOnPromptAccepted", () => {
  it("A08: 已结束清单在新请求被接受后自动收纳，历史记录保留", async () => {
    const finished = [
      { content: "done", activeForm: "doing done", status: "completed" },
      { content: "dropped", activeForm: "doing dropped", status: "cancelled" },
    ];
    const { engine, manager, sessionPath } = await seedSession(finished);

    dismissFinishedTodosOnPromptAccepted(engine, sessionPath);

    const snapshot = await loadLatestTodoSnapshotFromSessionFile(sessionPath);
    expect(snapshot).toMatchObject({ removed: true, dismissed: true });
    // 历史记录保留：完成/取消结果不被改写
    expect(snapshot?.todos).toEqual(finished);
    expect(engine.syncSessionBranchHead).toHaveBeenCalledWith(sessionPath, manager, "todo_auto_dismiss_append");
    expect(engine.emitEvent).toHaveBeenCalledWith(
      { type: "todo_update", removed: true, dismissed: true, todos: [] },
      sessionPath,
    );
  });

  it("A07: 仍有未完成项的清单不受新请求影响", async () => {
    const active = [
      { content: "done", activeForm: "doing done", status: "completed" },
      { content: "working", activeForm: "doing working", status: "in_progress" },
    ];
    const { engine, sessionPath } = await seedSession(active);

    dismissFinishedTodosOnPromptAccepted(engine, sessionPath);

    const snapshot = await loadLatestTodoSnapshotFromSessionFile(sessionPath);
    expect(snapshot).toMatchObject({ removed: false, finished: false, dismissed: false });
    expect(snapshot?.todos).toEqual(active);
    expect(engine.emitEvent).not.toHaveBeenCalled();
  });

  it("已收纳的清单不重复写记录", async () => {
    const finished = [{ content: "done", activeForm: "doing done", status: "completed" }];
    const { engine, sessionPath } = await seedSession(finished);

    dismissFinishedTodosOnPromptAccepted(engine, sessionPath);
    engine.emitEvent.mockClear();
    dismissFinishedTodosOnPromptAccepted(engine, sessionPath);

    expect(engine.emitEvent).not.toHaveBeenCalled();
  });
});
