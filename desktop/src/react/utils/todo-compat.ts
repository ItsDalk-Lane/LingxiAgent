/**
 * todo-compat.ts — 前端镜像
 *
 * 后端真实来源：lib/tools/todo-compat.ts
 * 这两个文件必须保持同步。任何改动都要改两处。
 *
 * 前端只镜像纯函数（migrateLegacyTodos / extractLatestTodos）。
 * branch-aware 的 loadLatestTodosFromSessionFile 是后端专用（Pi SDK 依赖），
 * 前端不需要。
 */

import { TODO_FORMAT_VERSION, TODO_STATE_CUSTOM_TYPE, TODO_TOOL_NAMES } from "./todo-constants";
import type { TodoItem, TodoStatus } from "../types";

type LegacyTodoItem = { id?: number; text: string; done: boolean };
type UnknownDetails = { todos?: unknown[] } & Record<string, unknown>;

const VALID_STATUSES: ReadonlySet<TodoStatus> = new Set<TodoStatus>([
  "pending",
  "in_progress",
  "blocked",
  "cancelled",
  "completed",
]);
const TERMINAL_STATUSES: ReadonlySet<TodoStatus> = new Set<TodoStatus>([
  "cancelled",
  "completed",
]);

function isLegacyTodoItem(item: unknown): item is LegacyTodoItem {
  return (
    typeof item === "object" &&
    item !== null &&
    typeof (item as { done?: unknown }).done === "boolean"
  );
}

function isNewTodoItem(item: unknown): item is TodoItem {
  if (typeof item !== "object" || item === null) return false;
  const it = item as Record<string, unknown>;
  return (
    typeof it.content === "string" &&
    typeof it.activeForm === "string" &&
    typeof it.status === "string" &&
    VALID_STATUSES.has(it.status as TodoStatus) &&
    (it.blockedReason === undefined || typeof it.blockedReason === "string")
  );
}

function migrateLegacyItem(old: LegacyTodoItem): TodoItem {
  return {
    content: old.text ?? "",
    activeForm: old.text ?? "",
    status: old.done ? "completed" : "pending",
  };
}

/**
 * 损坏 item 直接丢弃（记录 error），不回填空串，避免渲染出空白行。
 */
export function migrateLegacyTodos(details: UnknownDetails | null | undefined): TodoItem[] {
  if (!details || typeof details !== "object") return [];
  const todos = (details as { todos?: unknown }).todos;
  if (!Array.isArray(todos)) return [];
  const result: TodoItem[] = [];
  for (const item of todos) {
    if (isLegacyTodoItem(item)) {
      result.push(migrateLegacyItem(item));
      continue;
    }
    if (isNewTodoItem(item)) {
      result.push(item);
      continue;
    }
    console.error("[todo-compat] 丢弃损坏的 todo item:", item);
  }
  return result;
}

/**
 * Claude-style lifecycle (v1 旧语义): a todo group is removed once every item
 * is completed. Empty todos are also a removed/cleared group.
 * 仅适用于无版本标识的旧记录；v2 记录见 resolveSnapshotFlags。
 */
export function isTodoGroupRemoved(todos: TodoItem[]): boolean {
  if (!Array.isArray(todos)) return false;
  if (todos.length === 0) return true;
  return todos.every((item) => item.status === "completed");
}

export function applyTodoLifecycle(todos: TodoItem[]): TodoItem[] {
  return isTodoGroupRemoved(todos) ? [] : todos;
}

/** v2：全部条目处于终态（completed / cancelled）且非空 → 清单已结束 */
export function isTodoGroupFinished(todos: TodoItem[]): boolean {
  if (!Array.isArray(todos) || todos.length === 0) return false;
  return todos.every((item) => item && TERMINAL_STATUSES.has(item.status));
}

/**
 * 清单版本：对规范化后的条目内容做 FNV-1a 哈希。
 * 与后端 lib/tools/todo-compat.ts 逐字一致；同一清单内容在任何路径
 * （实时事件 / 历史恢复 / 服务端重算）得到同一版本号。用户收尾操作
 * 携带该版本，服务端据以识别"操作针对的是不是用户看见的那一版"（A17）。
 */
export function computeTodoListVersion(todos: TodoItem[] | null | undefined): string {
  const canonical = (Array.isArray(todos) ? todos : []).map((item) => [
    typeof item?.content === "string" ? item.content : "",
    typeof item?.activeForm === "string" ? item.activeForm : "",
    typeof item?.status === "string" ? item.status : "",
    typeof item?.blockedReason === "string" ? item.blockedReason : "",
  ]);
  const text = JSON.stringify(canonical);
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    // FNV prime multiplication via shifts (32-bit)
    hash = (hash + ((hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24))) >>> 0;
  }
  return `tv${hash.toString(16).padStart(8, "0")}`;
}

type MessageLike = { role?: string; toolName?: string; details?: unknown };
export type TodoSnapshot = {
  todos: TodoItem[];
  removed: boolean;
  finished: boolean;
  allCompleted: boolean;
  dismissed: boolean;
  format: 1 | 2;
  source: "tool" | "user";
  version: string;
};

function isValidTodoSnapshot(details: unknown): details is { todos: unknown[] } {
  return (
    !!details &&
    typeof details === "object" &&
    Array.isArray((details as { todos?: unknown }).todos)
  );
}

function snapshotFormat(details: unknown): 1 | 2 {
  return (details as { todoVersion?: unknown } | null | undefined)?.todoVersion === TODO_FORMAT_VERSION ? 2 : 1;
}

/**
 * 统一计算快照的展示标志。
 * v1（旧记录）：全 completed 或空 → removed（旧"已收纳"语义，不复活）。
 * v2：空 → removed（显式清空）；dismissed → removed（已收纳）；
 *     全部终态且未收纳 → finished（保留收尾摘要）。
 */
function resolveSnapshotFlags(details: unknown, todos: TodoItem[]): Pick<TodoSnapshot, "format" | "removed" | "dismissed" | "finished" | "allCompleted"> {
  const format = snapshotFormat(details);
  if (format === 2) {
    const dismissed = (details as { dismissed?: unknown } | null)?.dismissed === true;
    const removed = todos.length === 0 || dismissed;
    const finished = !removed && isTodoGroupFinished(todos);
    return {
      format,
      removed,
      dismissed,
      finished,
      allCompleted: finished && todos.every((item) => item.status === "completed"),
    };
  }
  return {
    format,
    removed: isTodoGroupRemoved(todos),
    dismissed: false,
    finished: false,
    allCompleted: false,
  };
}

function snapshotFromToolResult(m: MessageLike): TodoSnapshot | { invalid: true } {
  if (!isValidTodoSnapshot(m.details)) {
    console.error("[todo-compat] 跳过坏 todo 快照，继续向前扫描:", {
      toolName: m.toolName,
      details: m.details,
    });
    return { invalid: true };
  }
  const todos = migrateLegacyTodos(m.details as UnknownDetails);
  return {
    todos,
    ...resolveSnapshotFlags(m.details, todos),
    source: "tool",
    version: computeTodoListVersion(todos),
  };
}

function snapshotFromTodoStateMessage(m: MessageLike & { customType?: string }): TodoSnapshot | { invalid: true } | null {
  if (m.role !== "custom" || m.customType !== TODO_STATE_CUSTOM_TYPE) return null;
  const details = m.details as ({ removed?: unknown; source?: unknown; dismissed?: unknown } & UnknownDetails) | null | undefined;
  if (!isValidTodoSnapshot(details)) {
    console.error("[todo-compat] 跳过坏 todo state 事件，继续向前扫描:", {
      customType: m.customType,
      details,
    });
    return { invalid: true };
  }
  const stateDetails = details as ({ removed?: unknown; source?: unknown } & UnknownDetails);
  const todos = migrateLegacyTodos(stateDetails);
  const flags = resolveSnapshotFlags(stateDetails, todos);
  return {
    todos,
    // v1 旧记录额外保留显式 removed 标记的兼容语义
    ...(flags.format === 1
      ? { ...flags, removed: stateDetails.removed !== false || flags.removed }
      : flags),
    source: stateDetails.source === "model" ? "tool" : "user",
    version: computeTodoListVersion(todos),
  };
}

export function extractLatestTodoSnapshot(sourceMessages: (MessageLike & { customType?: string })[] | null | undefined): TodoSnapshot | null {
  if (!Array.isArray(sourceMessages)) return null;
  for (let i = sourceMessages.length - 1; i >= 0; i--) {
    const m = sourceMessages[i];
    if (!m) continue;

    const stateSnapshot = snapshotFromTodoStateMessage(m);
    if (stateSnapshot) {
      if ("invalid" in stateSnapshot) continue;
      return stateSnapshot;
    }

    if (m.role !== "toolResult") continue;
    if (!m.toolName || !TODO_TOOL_NAMES.includes(m.toolName as typeof TODO_TOOL_NAMES[number])) continue;
    const toolSnapshot = snapshotFromToolResult(m);
    if ("invalid" in toolSnapshot) continue;
    return toolSnapshot;
  }
  return null;
}

/**
 * 从后往前扫最后一个合法 todo 快照。坏快照（details.todos 缺失或非数组）
 * 跳过继续向前，这样显式清空（[]）和坏数据能区分开。
 */
export function extractLatestTodos(sourceMessages: MessageLike[] | null | undefined): TodoItem[] | null {
  const snapshot = extractLatestTodoSnapshot(sourceMessages);
  if (!snapshot) return null;
  // removed 已按格式版本覆盖旧"全完成即移除"与新"显式清空/已收纳"语义，
  // 不再叠加 applyTodoLifecycle，避免把 v2 收尾摘要误清空。
  return snapshot.removed ? [] : snapshot.todos;
}

/**
 * 从实时 tool_end 的 details 构建面板快照。
 * details 非法（缺失 / todos 非数组）返回 null —— 调用方必须保留
 * 最后一份有效清单并标记更新失败，绝不能转换为空清单（A13）。
 */
export function panelSnapshotFromToolDetails(details: unknown): TodoSnapshot | null {
  if (!isValidTodoSnapshot(details)) return null;
  const todos = migrateLegacyTodos(details as UnknownDetails);
  return {
    todos,
    ...resolveSnapshotFlags(details, todos),
    source: "tool",
    version: computeTodoListVersion(todos),
  };
}
