/**
 * 清单"单一事实源"三管道一致性回归（机制 1）。
 *
 * 同一份 todo_write 工具结果，三个消费管道必须产出语义一致的快照：
 * 1. 实时管道：WS tool_end.details → 前端 panelSnapshotFromToolDetails
 * 2. 历史恢复：会话消息 → 服务端/共享 extractLatestTodoSnapshot
 * 3. 分支重置：core 使用同一 extractLatestTodoSnapshot（与 2 同函数）
 *
 * 历史缺陷：实时广播丢 todoVersion → 前端误判为 v1 → 全完成清单被当作
 * 旧语义"全完成即移除"，与历史恢复管道（v2 收尾摘要）分裂。
 */
import { describe, it, expect } from "vitest";
import {
  extractLatestTodoSnapshot,
  computeTodoListVersion,
} from "../lib/tools/todo-compat.ts";
import { TODO_FORMAT_VERSION } from "../lib/tools/todo-constants.ts";
import { panelSnapshotFromToolDetails } from "../desktop/src/react/utils/todo-compat.ts";

function toolResultMessage(details: unknown, isError = false) {
  return {
    role: "toolResult",
    toolCallId: "call-1",
    toolName: "todo_write",
    content: [{ type: "text", text: "ok" }],
    isError,
    timestamp: Date.now(),
    details,
  };
}

/** 实时管道消费的事件 details（与服务端广播白名单透传后的形状一致）。 */
function liveBroadcastDetails(resultDetails: Record<string, unknown>) {
  return Object.fromEntries(
    ["todos", "todoVersion", "sessionFile", "sessionFileRef", "writableLocalRef"]
      .filter((key) => resultDetails[key] !== undefined)
      .map((key) => [key, resultDetails[key]]),
  );
}

const SEMANTIC_KEYS = ["todos", "removed", "dismissed", "finished", "allCompleted", "version", "format"] as const;

function semanticShape(snapshot: Record<string, unknown> | null) {
  if (!snapshot) return null;
  return Object.fromEntries(SEMANTIC_KEYS.map((key) => [key, snapshot[key]]));
}

describe("todo 三管道一致性", () => {
  const cases: Array<[string, unknown[]]> = [
    ["活动清单（混合状态）", [
      { content: "读 spec", activeForm: "正在读 spec", status: "completed" },
      { content: "改代码", activeForm: "正在改代码", status: "in_progress" },
      { content: "补测试", activeForm: "正在补测试", status: "pending" },
      { content: "部署", activeForm: "正在部署", status: "blocked", blockedReason: "缺少凭据" },
    ]],
    ["全部完成（v2 收尾摘要，不得按 v1 移除）", [
      { content: "读 spec", activeForm: "正在读 spec", status: "completed" },
      { content: "改代码", activeForm: "正在改代码", status: "completed" },
    ]],
    ["完成+取消混合终态", [
      { content: "读 spec", activeForm: "正在读 spec", status: "completed" },
      { content: "改代码", activeForm: "正在改代码", status: "cancelled" },
    ]],
    ["显式清空", []],
  ];

  for (const [name, todos] of cases) {
    it(`${name}：实时与历史快照逐字段一致`, () => {
      const resultDetails = { todos, todoVersion: TODO_FORMAT_VERSION };
      const fromHistory = extractLatestTodoSnapshot([toolResultMessage(resultDetails)]);
      const fromLive = panelSnapshotFromToolDetails(liveBroadcastDetails(resultDetails));
      expect(semanticShape(fromLive as unknown as Record<string, unknown> | null)).toEqual(
        semanticShape(fromHistory as unknown as Record<string, unknown> | null),
      );
      // 版本号必须一致（版本保护依赖跨管道稳定的哈希）
      if (todos.length > 0) {
        expect((fromLive as { version?: string })?.version).toBe(computeTodoListVersion(todos as never));
      }
    });
  }

  it("广播透传必须包含 todoVersion（回归：缺失即 v1 误判）", () => {
    const details = liveBroadcastDetails({ todos: [], todoVersion: TODO_FORMAT_VERSION });
    expect(details.todoVersion).toBe(TODO_FORMAT_VERSION);
  });
});
