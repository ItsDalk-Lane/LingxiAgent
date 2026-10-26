/**
 * todo-constants.ts — 前端镜像
 *
 * 后端真实来源：lib/tools/todo-constants.ts
 * 这两个文件必须保持同步。任何改动都要改两处。
 *
 * 命名不对称说明：tool 名用 snake_case（todo_write），对齐 SDK built-in
 * wire format；i18n key 用 camelCase (toolDef.todoWrite)，对齐 JS 属性命名
 * 惯例。跨领域的不对称是故意的。
 */

/** 新 tool 正式名（对标 Claude Code TodoWrite） */
export const TODO_WRITE_TOOL_NAME = "todo_write" as const;

/** 所有被识别为 todo 相关的 tool 名字 */
export const TODO_TOOL_NAMES = ["todo", TODO_WRITE_TOOL_NAME] as const;

export type TodoToolName = typeof TODO_TOOL_NAMES[number];

/** Hana 内部 session 事件：用户手动完成并移除当前 todo group */
export const TODO_STATE_CUSTOM_TYPE = "lingxi.todo_state" as const;

/**
 * 清单数据格式版本。details.todoVersion === TODO_FORMAT_VERSION 的记录
 * 采用新版生命周期：全部终态（完成/取消）保留收尾摘要，直到用户收纳或
 * 新一轮请求被正式接受；受阻/取消是合法状态。
 * 无版本标识的旧记录保持旧语义：全部 completed 即移除，不重新弹出。
 */
export const TODO_FORMAT_VERSION = 2;

/**
 * 按需目录桥（mcp_call）转发 todo_write 时，tool_end 事件/落档的 toolName 是
 * 桥本身，而非目标工具；仅按名筛选会让面板对整条清单失明。桥接结果按
 * 版本化载荷形状识别（details.todoVersion 为当前版本且 todos 为数组）。
 * Rust 内核事件以目标 id（tool:first-party:<name>）报名，剥前缀后同口径。
 * 与 lib/tools/todo-constants.ts 的 isTodoToolResult 同步。
 */
export const TOOL_CATALOG_CALL_TOOL_NAME = "mcp_call" as const;

export function isTodoToolResult(toolName: unknown, details: unknown): boolean {
  const name = typeof toolName === "string" ? toolName.replace(/^tool:first-party:/, "") : "";
  if ((TODO_TOOL_NAMES as readonly string[]).includes(name)) return true;
  if (name !== TOOL_CATALOG_CALL_TOOL_NAME) return false;
  if (!details || typeof details !== "object") return false;
  const record = details as Record<string, unknown>;
  // 桥接的 todo_write 拒绝结果（invalid_todos）同样属 todo 结果：据此保留
  // 最后有效清单并标记更新失败，而不是把它当成别的工具的失败。
  if (record.error === "invalid_todos") return true;
  return record.todoVersion === TODO_FORMAT_VERSION && Array.isArray(record.todos);
}

/** 全部合法 todo 状态（含 v2 新增的 blocked / cancelled） */
export const TODO_STATUSES = [
  "pending",
  "in_progress",
  "blocked",
  "cancelled",
  "completed",
] as const;

/** 终态集合：完成或取消都算"已结束"，不计入剩余工作 */
export const TODO_TERMINAL_STATUSES = ["cancelled", "completed"] as const;
