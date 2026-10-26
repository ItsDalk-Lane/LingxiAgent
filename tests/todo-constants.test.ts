/**
 * todo-constants.js — shared constants smoke test
 *
 * Verifies that the shared constants module exports the expected
 * names and that "todo" (legacy) and "todo_write" (new) are both present.
 */
import { describe, it, expect } from "vitest";
import { TODO_TOOL_NAMES, TODO_WRITE_TOOL_NAME, isTodoToolResult } from "../lib/tools/todo-constants.ts";
import { extractLatestTodoSnapshot } from "../lib/tools/todo-compat.ts";

describe("todo-constants", () => {
  it("exports TODO_TOOL_NAMES containing both legacy and new names", () => {
    expect(Array.isArray(TODO_TOOL_NAMES)).toBe(true);
    expect(TODO_TOOL_NAMES).toContain("todo");
    expect(TODO_TOOL_NAMES).toContain("todo_write");
    expect(TODO_TOOL_NAMES.length).toBe(2);
  });

  it("exports TODO_WRITE_TOOL_NAME as 'todo_write'", () => {
    expect(TODO_WRITE_TOOL_NAME).toBe("todo_write");
  });

  it("TODO_TOOL_NAMES is frozen (immutable)", () => {
    expect(Object.isFrozen(TODO_TOOL_NAMES)).toBe(true);
  });

  // R08 收尾：todo_write 非常驻、默认经 mcp_call 桥调用，tool_result 以桥名落档。
  describe("isTodoToolResult（名或版本化载荷形状）", () => {
    it("按名识别 todo / todo_write（载荷任意）", () => {
      expect(isTodoToolResult("todo_write", null)).toBe(true);
      expect(isTodoToolResult("todo", undefined)).toBe(true);
    });
    it("mcp_call 桥接的版本化 todo 载荷识别为 todo 结果", () => {
      expect(isTodoToolResult("mcp_call", { todoVersion: 2, todos: [] })).toBe(true);
      expect(isTodoToolResult("mcp_call", { error: "invalid_todos", reasons: ["x"] })).toBe(true);
    });
    it("mcp_call 转发其他工具的载荷不识别", () => {
      expect(isTodoToolResult("mcp_call", { checkpoint: "cp", createdAt: 1 })).toBe(false);
      expect(isTodoToolResult("mcp_call", { todos: [] })).toBe(false);
      expect(isTodoToolResult("mcp_call", null)).toBe(false);
    });
    it("其他工具即使带 todos 形状也不识别；Rust 目标 id 前缀剥离后同口径", () => {
      expect(isTodoToolResult("checkpoint", { todoVersion: 2, todos: [] })).toBe(false);
      expect(isTodoToolResult("tool:first-party:todo_write", null)).toBe(true);
      expect(isTodoToolResult("tool:first-party:mcp_call", { todoVersion: 2, todos: [] })).toBe(true);
    });
    it("extractLatestTodoSnapshot 读到 mcp_call 桥接写入的清单", () => {
      const todos = [{ content: "桥接项", activeForm: "桥接项", status: "pending" }];
      const snapshot = extractLatestTodoSnapshot([
        { role: "user", content: "x" },
        { role: "toolResult", toolName: "mcp_call", details: { todoVersion: 2, todos } },
        { role: "toolResult", toolName: "mcp_call", details: { checkpoint: "cp-1" } },
      ]);
      expect(snapshot?.todos.map((t) => t.content)).toEqual(["桥接项"]);
      expect(snapshot?.removed).toBe(false);
    });
  });
});
