/**
 * 轮次注入（融合方案机制 2）：buildTodoContextBlockForPrompt。
 *
 * - 存在未完成清单：生成含快照版本、逐项状态、受阻原因的注入文本，
 *   并附带"in_progress 不一定在运行"的核实指引。
 * - 已结束 / 已收纳 / 无清单：返回 null（已结束走自动收纳通道，互斥）。
 * - 注入文本来自持久化快照，跨中断/重启保持正确。
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import fs from "fs/promises";
import path from "path";
import os from "os";
import { buildTodoContextBlockForPrompt } from "../server/routes/chat.ts";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import { TODO_FORMAT_VERSION, TODO_STATE_CUSTOM_TYPE } from "../lib/tools/todo-constants.ts";

let tmpDir: string;

beforeEach(async () => {
  tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "todo-prompt-inject-"));
});

afterEach(async () => {
  await fs.rm(tmpDir, { recursive: true, force: true });
});

async function seedSession(todos: any[], opts: { dismiss?: boolean } = {}) {
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
  if (opts.dismiss) {
    manager.appendCustomMessageEntry(TODO_STATE_CUSTOM_TYPE, "[Hana Todo] dismissed", false, {
      action: "dismiss",
      source: "system",
      todoVersion: TODO_FORMAT_VERSION,
      removed: true,
      dismissed: true,
      todos,
    });
  }
  const engine = {
    getSessionByPath: vi.fn(() => null),
    openSessionManagerAtCurrentBranch: vi.fn(() => manager),
  };
  return { engine, sessionPath };
}

const UNFINISHED = [
  { content: "读 spec", activeForm: "正在读 spec", status: "completed" },
  { content: "改代码", activeForm: "正在改代码", status: "in_progress" },
  { content: "部署", activeForm: "正在部署", status: "blocked", blockedReason: "缺少凭据" },
];

describe("buildTodoContextBlockForPrompt", () => {
  it("未完成清单：生成含版本、逐项状态与受阻原因的注入文本", async () => {
    const { engine, sessionPath } = await seedSession(UNFINISHED);
    const block = buildTodoContextBlockForPrompt(engine, sessionPath);

    expect(block).toBeTruthy();
    expect(block).toContain("[Hana Todo] Authoritative task list state");
    expect(block).toMatch(/snapshot tv[0-9a-f]{8}/);
    expect(block).toContain("1. [completed] 读 spec");
    expect(block).toContain("2. [in_progress] 改代码");
    expect(block).toContain("3. [blocked] 部署 (blocked: 缺少凭据)");
    // 中断核实指引：in_progress 不一定在运行
    expect(block).toMatch(/not necessarily running/i);
    // 同一状态重复调用产生同一文本（幂等，内容哈希驱动）
    expect(buildTodoContextBlockForPrompt(engine, sessionPath)).toBe(block);
  });

  it("全部完成（已结束）：返回 null，走自动收纳通道", async () => {
    const { engine, sessionPath } = await seedSession([
      { content: "done", activeForm: "doing done", status: "completed" },
    ]);
    expect(buildTodoContextBlockForPrompt(engine, sessionPath)).toBeNull();
  });

  it("已收纳：返回 null", async () => {
    const { engine, sessionPath } = await seedSession(UNFINISHED, { dismiss: true });
    expect(buildTodoContextBlockForPrompt(engine, sessionPath)).toBeNull();
  });

  it("没有清单历史：返回 null", async () => {
    const sessionDir = path.join(tmpDir, "agents", "hana", "sessions");
    const manager = SessionManager.create("/tmp/workspace", sessionDir);
    const sessionPath = manager.getSessionFile();
    const engine = { getSessionByPath: vi.fn(() => null), openSessionManagerAtCurrentBranch: vi.fn(() => manager) };
    expect(buildTodoContextBlockForPrompt(engine, sessionPath)).toBeNull();
  });

  it("sessionPath 缺失或管理器不可用：返回 null 而不抛错", () => {
    expect(buildTodoContextBlockForPrompt({}, null)).toBeNull();
    expect(buildTodoContextBlockForPrompt({ getSessionByPath: () => null }, "/nonexistent/x.jsonl")).toBeNull();
  });
});
