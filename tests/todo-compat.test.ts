/**
 * todo-compat.js — legacy format migration pure function tests
 *
 * Covers:
 * - migrateLegacyTodos: old {id,text,done} → new {content,activeForm,status}
 * - migrateLegacyTodos: new format passthrough (idempotent)
 * - migrateLegacyTodos: edge cases (null, undefined, empty)
 * - migrateLegacyTodos: 丢弃损坏 item（不再 sanitize 成空串）
 * - extractLatestTodos: scan sourceMessages for latest todo tool_result
 * - extractLatestTodos: both "todo" and "todo_write" tool names
 * - extractLatestTodos: legacy format auto-converts
 * - extractLatestTodos: 坏快照（details.todos 缺失/非数组）继续向前扫
 * - extractLatestTodosFromEntries: branch-aware leaf-path 解析
 */
import { describe, it, expect, vi } from "vitest";
import {
  migrateLegacyTodos,
  extractLatestTodos,
  extractLatestTodosFromEntries,
  extractLatestTodoSnapshot,
} from "../lib/tools/todo-compat.ts";

describe("migrateLegacyTodos", () => {
  it("converts legacy {id, text, done: false} to pending", () => {
    const legacy = {
      action: "add",
      todos: [{ id: 1, text: "读取 spec", done: false }],
      nextId: 2,
    };
    const result = migrateLegacyTodos(legacy);
    expect(result).toEqual([
      { content: "读取 spec", activeForm: "读取 spec", status: "pending" },
    ]);
  });

  it("converts legacy {id, text, done: true} to completed", () => {
    const legacy = {
      action: "toggle",
      todos: [{ id: 1, text: "读取 spec", done: true }],
      nextId: 2,
    };
    const result = migrateLegacyTodos(legacy);
    expect(result).toEqual([
      { content: "读取 spec", activeForm: "读取 spec", status: "completed" },
    ]);
  });

  it("passes through new format unchanged (idempotent)", () => {
    const newFormat = {
      todos: [
        { content: "分析", activeForm: "正在分析", status: "in_progress" },
      ],
    };
    const result = migrateLegacyTodos(newFormat);
    expect(result).toEqual([
      { content: "分析", activeForm: "正在分析", status: "in_progress" },
    ]);
  });

  it("handles empty todos array", () => {
    expect(migrateLegacyTodos({ todos: [] })).toEqual([]);
  });

  it("returns [] for null/undefined details", () => {
    expect(migrateLegacyTodos(null)).toEqual([]);
    expect(migrateLegacyTodos(undefined)).toEqual([]);
    expect(migrateLegacyTodos({})).toEqual([]);
  });

  it("returns [] when todos field is missing", () => {
    expect(migrateLegacyTodos({ action: "list" })).toEqual([]);
  });

  it("handles mixed legacy + partial new-format items safely", () => {
    const mixed = {
      todos: [
        { id: 1, text: "legacy", done: false },
        { content: "new", activeForm: "正在 new", status: "pending" },
      ],
    };
    const result = migrateLegacyTodos(mixed);
    expect(result).toHaveLength(2);
    expect(result[0]).toEqual({ content: "legacy", activeForm: "legacy", status: "pending" });
    expect(result[1]).toEqual({ content: "new", activeForm: "正在 new", status: "pending" });
  });

  it("丢弃损坏 item（不回填空串，避免空白行）", () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const garbage = {
      todos: [
        { foo: "bar" },                                   // 不合法
        { content: "has content", status: "bogus_state" }, // 非法 status
        null,                                              // null 直接丢
        { content: "valid", activeForm: "正在 valid", status: "pending" }, // 合法
      ],
    };
    const result = migrateLegacyTodos(garbage);
    // 只保留合法的那一条
    expect(result).toHaveLength(1);
    expect(result[0]).toEqual({
      content: "valid",
      activeForm: "正在 valid",
      status: "pending",
    });
    // 三条损坏 item 都记录了 error
    expect(errorSpy).toHaveBeenCalledTimes(3);
    errorSpy.mockRestore();
  });

  it("全部损坏时返回空数组", () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const allGarbage = { todos: [{ foo: 1 }, { bar: 2 }] };
    const result = migrateLegacyTodos(allGarbage);
    expect(result).toEqual([]);
    errorSpy.mockRestore();
  });

  it("丢弃不可 JSON 序列化的损坏 item 时不抛错", () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const circular: any = { foo: "bar" };
    circular.self = circular;
    let result;

    expect(() => {
      result = migrateLegacyTodos({ todos: [circular] });
    }).not.toThrow();
    expect(result).toEqual([]);
    expect(errorSpy).toHaveBeenCalledTimes(1);
    expect(errorSpy.mock.calls[0][0]).toContain("[Circular]");

    errorSpy.mockRestore();
  });
});

describe("extractLatestTodos", () => {
  it("returns null when no todo tool result in messages", () => {
    const messages = [
      { role: "user", content: "hi" },
      { role: "assistant", content: "hello" },
    ];
    expect(extractLatestTodos(messages)).toBe(null);
  });

  it("finds last toolResult with toolName='todo' (legacy)", () => {
    const messages = [
      { role: "user", content: "task" },
      {
        role: "toolResult",
        toolName: "todo",
        details: {
          action: "add",
          todos: [{ id: 1, text: "step1", done: false }],
          nextId: 2,
        },
      },
    ];
    const result = extractLatestTodos(messages);
    expect(result).toEqual([
      { content: "step1", activeForm: "step1", status: "pending" },
    ]);
  });

  it("finds last toolResult with toolName='todo_write' (new)", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todos: [
            { content: "step1", activeForm: "正在 step1", status: "in_progress" },
          ],
        },
      },
    ];
    const result = extractLatestTodos(messages);
    expect(result).toEqual([
      { content: "step1", activeForm: "正在 step1", status: "in_progress" },
    ]);
  });

  it("returns only the latest when multiple todo tool results exist", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "todo",
        details: { todos: [{ id: 1, text: "old", done: false }], nextId: 2 },
      },
      {
        role: "toolResult",
        toolName: "todo_write",
        details: { todos: [{ content: "new", activeForm: "正在 new", status: "pending" }] },
      },
    ];
    const result = extractLatestTodos(messages);
    expect(result).toEqual([
      { content: "new", activeForm: "正在 new", status: "pending" },
    ]);
  });

  it("skips non-toolResult entries", () => {
    const messages = [
      { role: "assistant", content: "..." },
      {
        role: "toolResult",
        toolName: "todo",
        details: { todos: [{ id: 1, text: "x", done: false }], nextId: 2 },
      },
      { role: "user", content: "..." },
    ];
    const result = extractLatestTodos(messages);
    expect(result).not.toBe(null);
    expect(result).toHaveLength(1);
  });

  it("ignores toolResult with other tool names", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "read",
        details: { content: "file" },
      },
    ];
    expect(extractLatestTodos(messages)).toBe(null);
  });

  it("显式空列表（todos: []）被当作合法清空快照返回", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todos: [{ content: "old", activeForm: "正在 old", status: "pending" }],
        },
      },
      {
        role: "toolResult",
        toolName: "todo_write",
        details: { todos: [] }, // 显式清空
      },
    ];
    const result = extractLatestTodos(messages);
    expect(result).toEqual([]);
  });

  it("全 completed 的旧格式（v1）todo group 按旧生命周期移除", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todos: [
            { content: "read", activeForm: "reading", status: "completed" },
            { content: "write", activeForm: "writing", status: "completed" },
          ],
        },
      },
    ];

    expect(extractLatestTodos(messages)).toEqual([]);
    const snapshot = extractLatestTodoSnapshot(messages);
    expect(snapshot).toMatchObject({
      todos: [
        { content: "read", activeForm: "reading", status: "completed" },
        { content: "write", activeForm: "writing", status: "completed" },
      ],
      removed: true,
      finished: false,
      dismissed: false,
      format: 1,
      source: "tool",
    });
    expect(typeof snapshot?.version).toBe("string");
  });

  it("用户完成事件覆盖旧 todo 快照并移除当前面板", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todos: [
            { content: "old", activeForm: "doing old", status: "in_progress" },
          ],
        },
      },
      {
        role: "custom",
        customType: "lingxi.todo_state",
        details: {
          action: "complete_all",
          removed: true,
          todos: [
            { content: "old", activeForm: "doing old", status: "completed" },
          ],
        },
      },
    ];

    expect(extractLatestTodos(messages)).toEqual([]);
    expect(extractLatestTodoSnapshot(messages)).toMatchObject({
      todos: [
        { content: "old", activeForm: "doing old", status: "completed" },
      ],
      removed: true,
      source: "user",
    });
  });

  it("用户完成事件之后的新 todo_write 会重新成为当前 active todo", () => {
    const messages = [
      {
        role: "custom",
        customType: "lingxi.todo_state",
        details: {
          action: "complete_all",
          removed: true,
          todos: [
            { content: "old", activeForm: "doing old", status: "completed" },
          ],
        },
      },
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todos: [
            { content: "new", activeForm: "doing new", status: "pending" },
          ],
        },
      },
    ];

    expect(extractLatestTodos(messages)).toEqual([
      { content: "new", activeForm: "doing new", status: "pending" },
    ]);
  });

  it("坏快照（details 缺失）跳过继续向前找到合法快照", () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todos: [{ content: "valid old", activeForm: "正在 valid old", status: "pending" }],
        },
      },
      {
        role: "toolResult",
        toolName: "todo_write",
        details: null, // 坏快照：details 为 null
      },
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {}, // 坏快照：details.todos 缺失
      },
    ];
    const result = extractLatestTodos(messages);
    expect(result).toEqual([
      { content: "valid old", activeForm: "正在 valid old", status: "pending" },
    ]);
    expect(errorSpy).toHaveBeenCalled();
    errorSpy.mockRestore();
  });

  it("坏快照（details.todos 非数组）跳过", () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: { todos: [{ content: "good", activeForm: "正在 good", status: "pending" }] },
      },
      {
        role: "toolResult",
        toolName: "todo_write",
        details: { todos: "not an array" },
      },
    ];
    const result = extractLatestTodos(messages);
    expect(result).toEqual([
      { content: "good", activeForm: "正在 good", status: "pending" },
    ]);
    errorSpy.mockRestore();
  });

  it("跳过不可 JSON 序列化的坏快照时不抛错", () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const badToolDetails: any = { todos: "not array" };
    badToolDetails.self = badToolDetails;
    const badStateDetails: any = { todos: "also not array" };
    badStateDetails.self = badStateDetails;
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: { todos: [{ content: "good", activeForm: "正在 good", status: "pending" }] },
      },
      {
        role: "toolResult",
        toolName: "todo_write",
        details: badToolDetails,
      },
      {
        role: "custom",
        customType: "lingxi.todo_state",
        details: badStateDetails,
      },
    ];
    let result;

    expect(() => {
      result = extractLatestTodos(messages);
    }).not.toThrow();
    expect(result).toEqual([
      { content: "good", activeForm: "正在 good", status: "pending" },
    ]);
    expect(errorSpy).toHaveBeenCalledTimes(2);
    expect(errorSpy.mock.calls[0][0]).toContain("[Circular]");

    errorSpy.mockRestore();
  });
});

describe("v2 格式快照语义（清单改版）", () => {
  it("v2 全部 completed → finished 收尾摘要保留，不按旧语义移除", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todoVersion: 2,
          todos: [
            { content: "read", activeForm: "reading", status: "completed" },
            { content: "write", activeForm: "writing", status: "completed" },
          ],
        },
      },
    ];
    const snapshot = extractLatestTodoSnapshot(messages);
    expect(snapshot).toMatchObject({
      removed: false,
      finished: true,
      allCompleted: true,
      dismissed: false,
      format: 2,
      source: "tool",
    });
    // extractLatestTodos 对收尾摘要返回清单本身（面板继续显示）
    expect(extractLatestTodos(messages)).toHaveLength(2);
  });

  it("v2 完成+取消混合 → finished 但不是 allCompleted", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todoVersion: 2,
          todos: [
            { content: "done", activeForm: "doing done", status: "completed" },
            { content: "dropped", activeForm: "doing dropped", status: "cancelled" },
          ],
        },
      },
    ];
    const snapshot = extractLatestTodoSnapshot(messages);
    expect(snapshot).toMatchObject({ removed: false, finished: true, allCompleted: false });
  });

  it("v2 blocked 条目不是终态，清单保持活动并保留受阻原因", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todoVersion: 2,
          todos: [
            { content: "done", activeForm: "doing done", status: "completed" },
            { content: "stuck", activeForm: "doing stuck", status: "blocked", blockedReason: "缺少凭据" },
          ],
        },
      },
    ];
    const snapshot = extractLatestTodoSnapshot(messages);
    expect(snapshot).toMatchObject({ removed: false, finished: false });
    expect(snapshot?.todos[1]).toMatchObject({ status: "blocked", blockedReason: "缺少凭据" });
  });

  it("v2 显式空清单 → removed（明确清空，与失败区分）", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: { todoVersion: 2, todos: [] },
      },
    ];
    expect(extractLatestTodoSnapshot(messages)).toMatchObject({
      removed: true, finished: false, dismissed: false, format: 2,
    });
    expect(extractLatestTodos(messages)).toEqual([]);
  });

  it("v2 用户取消事件：未完成项已取消，保持收尾摘要可见", () => {
    const messages = [
      {
        role: "custom",
        customType: "lingxi.todo_state",
        details: {
          action: "cancel_remaining",
          source: "user",
          todoVersion: 2,
          removed: false,
          dismissed: false,
          todos: [
            { content: "done", activeForm: "doing done", status: "completed" },
            { content: "rest", activeForm: "doing rest", status: "cancelled" },
          ],
        },
      },
    ];
    expect(extractLatestTodoSnapshot(messages)).toMatchObject({
      removed: false, finished: true, allCompleted: false, source: "user",
    });
  });

  it("v2 收纳事件 → removed + dismissed（重开会话不重新弹出）", () => {
    const messages = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todoVersion: 2,
          todos: [{ content: "done", activeForm: "doing done", status: "completed" }],
        },
      },
      {
        role: "custom",
        customType: "lingxi.todo_state",
        details: {
          action: "dismiss",
          source: "user",
          todoVersion: 2,
          removed: true,
          dismissed: true,
          todos: [{ content: "done", activeForm: "doing done", status: "completed" }],
        },
      },
    ];
    expect(extractLatestTodoSnapshot(messages)).toMatchObject({
      removed: true, dismissed: true, finished: false, source: "user",
    });
    expect(extractLatestTodos(messages)).toEqual([]);
  });

  it("旧手动全部完成记录（无版本标识、removed:true）不复活", () => {
    // 旧消息样本：改版前的手动完成记录
    const messages = [
      {
        role: "custom",
        customType: "lingxi.todo_state",
        details: {
          action: "complete_all",
          source: "user",
          removed: true,
          todos: [
            { content: "old", activeForm: "doing old", status: "completed" },
          ],
        },
      },
    ];
    const snapshot = extractLatestTodoSnapshot(messages);
    expect(snapshot).toMatchObject({
      removed: true, finished: false, dismissed: false, format: 1, source: "user",
    });
    expect(extractLatestTodos(messages)).toEqual([]);
  });

  it("同一清单内容算出同一版本号，内容变化版本号变化（A17 依据）", () => {
    const base = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todoVersion: 2,
          todos: [{ content: "a", activeForm: "doing a", status: "pending" }],
        },
      },
    ];
    const changed = [
      {
        role: "toolResult",
        toolName: "todo_write",
        details: {
          todoVersion: 2,
          todos: [
            { content: "a", activeForm: "doing a", status: "pending" },
            { content: "b", activeForm: "doing b", status: "pending" },
          ],
        },
      },
    ];
    const v1 = extractLatestTodoSnapshot(base)?.version;
    expect(v1).toMatch(/^tv[0-9a-f]{8}$/);
    expect(extractLatestTodoSnapshot(base)?.version).toBe(v1);
    expect(extractLatestTodoSnapshot(changed)?.version).not.toBe(v1);
  });
});

describe("extractLatestTodosFromEntries (branch-aware)", () => {  // 构造一个分叉 session：
  //   session header (S)
  //   message#1 (parent: null) — user
  //   message#2 (parent: #1) — assistant
  //   todoResult#3 (parent: #2) — toolResult todo_write: 分支 A 的状态
  //   message#4 (parent: #2) — user 在 #2 后开了新分支（parent=#2 而不是 #3）
  //   message#5 (parent: #4) — assistant
  //   todoResult#6 (parent: #5) — toolResult todo_write: 分支 B 的状态（当前 leaf）
  //
  // 物理文件顺序是 S,#1,#2,#3,#4,#5,#6，但当前 leaf 是 #6，
  // 沿 parent 回溯：#6 -> #5 -> #4 -> #2 -> #1 -> root。
  // 分支 A 的 #3 不在 leaf path 上，应该被忽略。
  it("只在 leaf path 上找最新 todo（忽略被抛弃的分支）", () => {
    const entries = [
      { type: "session", id: "sess-1", version: 3, timestamp: "2026-04-13T00:00:00.000Z", cwd: "/tmp" },
      {
        type: "message",
        id: "m1",
        parentId: null,
        timestamp: "2026-04-13T00:00:01.000Z",
        message: { role: "user", content: "start" },
      },
      {
        type: "message",
        id: "m2",
        parentId: "m1",
        timestamp: "2026-04-13T00:00:02.000Z",
        message: { role: "assistant", content: "ok" },
      },
      {
        type: "message",
        id: "m3",
        parentId: "m2",
        timestamp: "2026-04-13T00:00:03.000Z",
        message: {
          role: "toolResult",
          toolName: "todo_write",
          details: {
            todos: [{ content: "branch A", activeForm: "正在 branch A", status: "pending" }],
          },
        },
      },
      {
        type: "message",
        id: "m4",
        parentId: "m2", // 新分支从 m2 分出
        timestamp: "2026-04-13T00:00:04.000Z",
        message: { role: "user", content: "switch" },
      },
      {
        type: "message",
        id: "m5",
        parentId: "m4",
        timestamp: "2026-04-13T00:00:05.000Z",
        message: { role: "assistant", content: "ok2" },
      },
      {
        type: "message",
        id: "m6",
        parentId: "m5",
        timestamp: "2026-04-13T00:00:06.000Z",
        message: {
          role: "toolResult",
          toolName: "todo_write",
          details: {
            todos: [{ content: "branch B", activeForm: "正在 branch B", status: "in_progress" }],
          },
        },
      },
    ];
    const result = extractLatestTodosFromEntries(entries);
    expect(result).toEqual([
      { content: "branch B", activeForm: "正在 branch B", status: "in_progress" },
    ]);
  });

  it("空 entries 返回 null", () => {
    expect(extractLatestTodosFromEntries([])).toBe(null);
    expect(extractLatestTodosFromEntries(null)).toBe(null);
    expect(extractLatestTodosFromEntries(undefined)).toBe(null);
  });

  it("无 session header 返回 null", () => {
    const entries = [
      {
        type: "message",
        id: "m1",
        parentId: null,
        timestamp: "2026-04-13T00:00:00.000Z",
        message: { role: "user", content: "x" },
      },
    ];
    expect(extractLatestTodosFromEntries(entries)).toBe(null);
  });
});
