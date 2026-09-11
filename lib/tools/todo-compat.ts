/**
 * todo-compat.js — 旧格式 → 新格式转换的纯函数
 *
 * 无状态、无副作用。前端镜像：desktop/src/react/utils/todo-compat.ts，
 * 必须保持同步。
 *
 * 旧格式（Pi SDK example 移植版）：
 *   { action, todos: [{id, text, done}], nextId }
 *
 * 新格式（对标 Claude Code TodoWrite）：
 *   { todos: [{content, activeForm, status}], warning? }
 *
 * 健壮性约定：
 * - 损坏 item（既不是 legacy 也不是 new）直接丢弃，不再 sanitize 成空串
 *   item。空字符串 item 渲染会导致空白行，污染 UI。
 * - extractLatestTodos 遇到缺失 / 非数组 details.todos 的 tool_result 视为
 *   "坏快照"，继续向前扫描。只有 details.todos 是数组（空数组也算）时才
 *   视为合法快照并返回。这样显式清空（todos: []）和坏数据能区分开。
 */

import fs from "fs/promises";
import { parseSessionEntries, buildSessionContext } from "../pi-sdk/index.ts";
import { TODO_FORMAT_VERSION, TODO_STATE_CUSTOM_TYPE, TODO_TOOL_NAMES } from "./todo-constants.ts";
import { createModuleLogger } from "../debug-log.ts";
import { redactLogValue } from "../log-redactor.ts";

const log = createModuleLogger("todo-compat");

const VALID_STATUSES = new Set(["pending", "in_progress", "blocked", "cancelled", "completed"]);
const TERMINAL_STATUSES = new Set(["cancelled", "completed"]);

function formatTodoDiagnostic(value) {
  const redacted = redactLogValue(value);
  try {
    const serialized = JSON.stringify(redacted);
    return serialized === undefined ? String(redacted) : serialized;
  } catch {
    return String(redacted);
  }
}

function isLegacyTodoItem(item) {
  return item && typeof item === "object" && typeof item.done === "boolean";
}

function isNewTodoItem(item) {
  return (
    item &&
    typeof item === "object" &&
    typeof item.content === "string" &&
    typeof item.activeForm === "string" &&
    VALID_STATUSES.has(item.status) &&
    (item.blockedReason === undefined || typeof item.blockedReason === "string")
  );
}

function migrateLegacyItem(old) {
  return {
    content: old.text ?? "",
    activeForm: old.text ?? "",  // decision 3: fallback 到 content
    status: old.done ? "completed" : "pending",
  };
}

/**
 * 把 details.todos 数组转成新格式数组。
 * 损坏 item 直接丢弃（记录 error），不回填。
 */
export function migrateLegacyTodos(details) {
  if (!details || typeof details !== "object") return [];
  const todos = details.todos;
  if (!Array.isArray(todos)) return [];
  const result = [];
  for (const item of todos) {
    if (isLegacyTodoItem(item)) {
      result.push(migrateLegacyItem(item));
      continue;
    }
    if (isNewTodoItem(item)) {
      result.push(item);
      continue;
    }
    log.error(`丢弃损坏的 todo item: ${formatTodoDiagnostic(item)}`);
  }
  return result;
}

/**
 * Claude-style lifecycle (v1 旧语义): a todo group is removed once every item
 * is completed. Empty todos are also a removed/cleared group.
 * 仅适用于无版本标识的旧记录；v2 记录见 resolveSnapshotFlags。
 */
export function isTodoGroupRemoved(todos) {
  if (!Array.isArray(todos)) return false;
  if (todos.length === 0) return true;
  return todos.every((item) => item.status === "completed");
}

export function applyTodoLifecycle(todos) {
  return isTodoGroupRemoved(todos) ? [] : todos;
}

/** v2：全部条目处于终态（completed / cancelled）且非空 → 清单已结束 */
export function isTodoGroupFinished(todos) {
  if (!Array.isArray(todos) || todos.length === 0) return false;
  return todos.every((item) => item && TERMINAL_STATUSES.has(item.status));
}

/**
 * 清单版本：对规范化后的条目内容做 FNV-1a 哈希。
 * 纯函数、无状态，前后端镜像必须逐字一致；同一清单内容在任何路径
 * （实时事件 / 历史恢复 / 服务端重算）得到同一版本号。用户收尾操作
 * 携带该版本，服务端据以识别"操作针对的是不是用户看见的那一版"（A17）。
 */
export function computeTodoListVersion(todos) {
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

function snapshotFormat(details) {
  return details && details.todoVersion === TODO_FORMAT_VERSION ? 2 : 1;
}

/**
 * 统一计算快照的展示标志。
 * v1（旧记录）：全 completed 或空 → removed（旧"已收纳"语义，不复活）。
 * v2：空 → removed（显式清空）；dismissed → removed（已收纳）；
 *     全部终态且未收纳 → finished（保留收尾摘要）。
 */
function resolveSnapshotFlags(details, todos) {
  const format = snapshotFormat(details);
  if (format === 2) {
    const dismissed = details?.dismissed === true;
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

/**
 * 从快照构建 todo_update 广播负载 / REST 响应 panel / hydrate todoPanel。
 * 快照缺失、已移除或已收纳 → removed 负载（当前展示隐藏，历史保留）。
 */
export function todoPanelPayloadFromSnapshot(snapshot) {
  const todos = snapshot && !snapshot.removed && Array.isArray(snapshot.todos)
    ? snapshot.todos
    : [];
  if (!todos.length) {
    return { removed: true, dismissed: snapshot?.dismissed === true, todos: [] };
  }
  return {
    todos,
    version: typeof snapshot.version === "string" ? snapshot.version : computeTodoListVersion(todos),
    finished: snapshot.finished === true,
    allCompleted: snapshot.allCompleted === true,
    dismissed: false,
  };
}

/**
 * 判定一个 tool_result 的 details 是否是合法的 todo 快照。
 * 合法 = details.todos 存在且是数组。空数组算合法（显式清空）。
 */
function isValidTodoSnapshot(details) {
  return !!details
    && typeof details === "object"
    && Array.isArray(details.todos);
}

function snapshotFromToolResult(m) {
  if (!isValidTodoSnapshot(m.details)) {
    log.error(`跳过坏 todo 快照，继续向前扫描: ${formatTodoDiagnostic({
      toolName: m.toolName,
      details: m.details,
    })}`);
    return { invalid: true };
  }
  const todos = migrateLegacyTodos(m.details);
  return {
    todos,
    ...resolveSnapshotFlags(m.details, todos),
    source: "tool",
    version: computeTodoListVersion(todos),
  };
}

function snapshotFromTodoStateMessage(m) {
  if (m.role !== "custom" || m.customType !== TODO_STATE_CUSTOM_TYPE) return null;
  const details = m.details;
  if (!isValidTodoSnapshot(details)) {
    log.error(`跳过坏 todo state 事件，继续向前扫描: ${formatTodoDiagnostic({
      customType: m.customType,
      details,
    })}`);
    return { invalid: true };
  }
  const todos = migrateLegacyTodos(details);
  const flags = resolveSnapshotFlags(details, todos);
  return {
    todos,
    // v1 旧记录额外保留显式 removed 标记的兼容语义
    ...(flags.format === 1
      ? { ...flags, removed: details.removed !== false || flags.removed }
      : flags),
    source: details.source === "model" ? "tool" : "user",
    version: computeTodoListVersion(todos),
  };
}

export interface TodoSnapshot {
  todos: any[];
  removed: boolean;
  finished: boolean;
  allCompleted: boolean;
  dismissed: boolean;
  format: number;
  source: "tool" | "user";
  version: string;
}

/**
 * 从后往前找最后一个合法 todo 快照；坏快照跳过继续向前。
 */
export function extractLatestTodoSnapshot(sourceMessages): TodoSnapshot | null {
  if (!Array.isArray(sourceMessages)) return null;
  for (let i = sourceMessages.length - 1; i >= 0; i--) {
    const m = sourceMessages[i];
    if (!m) continue;

    const stateSnapshot = snapshotFromTodoStateMessage(m);
    if (stateSnapshot) {
      if (stateSnapshot.invalid) continue;
      return stateSnapshot as TodoSnapshot;
    }

    if (m.role !== "toolResult") continue;
    if (!TODO_TOOL_NAMES.includes(m.toolName)) continue;
    const toolSnapshot = snapshotFromToolResult(m);
    if (toolSnapshot.invalid) continue;
    return toolSnapshot as TodoSnapshot;
  }
  return null;
}

/**
 * 在一个线性消息数组中从后往前找最后一个合法 todo 快照。
 * 坏快照（details.todos 缺失或非数组）跳过继续向前。
 */
export function extractLatestTodos(sourceMessages) {
  const snapshot = extractLatestTodoSnapshot(sourceMessages);
  if (!snapshot) return null;
  // removed 已按格式版本覆盖旧"全完成即移除"与新"显式清空/已收纳"语义，
  // 不再叠加 applyTodoLifecycle，避免把 v2 收尾摘要误清空。
  return snapshot.removed ? [] : snapshot.todos;
}

/**
 * Branch-aware：从 session entries 沿当前 leaf 回溯到 root，
 * 只在当前分支路径上扫描最新 todo 快照。
 *
 * Pi SDK session 是 parent/child 树，file 物理顺序可能包含被抛弃的分支，
 * 直接按 file 顺序扫会取到错误分支的状态。必须先用 buildSessionContext
 * 走 leaf-to-root 路径。
 */
export function extractLatestTodosFromEntries(entries) {
  if (!Array.isArray(entries) || entries.length === 0) return null;
  const header = entries[0];
  if (!header || header.type !== "session") return null;
  const { messages } = buildSessionContext(entries);
  return extractLatestTodos(messages);
}

export function extractLatestTodoSnapshotFromEntries(entries) {
  if (!Array.isArray(entries) || entries.length === 0) return null;
  const header = entries[0];
  if (!header || header.type !== "session") return null;
  const { messages } = buildSessionContext(entries);
  return extractLatestTodoSnapshot(messages);
}

/**
 * 从一个 session 文件读取 entries 并提取 branch-aware 的最新 todos。
 * 文件读取失败或无有效 header 返回 null。
 */
export async function loadLatestTodosFromSessionFile(sessionPath) {
  if (!sessionPath) return null;
  try {
    const raw = await fs.readFile(sessionPath, "utf-8");
    const entries = parseSessionEntries(raw);
    return extractLatestTodosFromEntries(entries);
  } catch {
    return null;
  }
}

export async function loadLatestTodoSnapshotFromSessionFile(sessionPath) {
  if (!sessionPath) return null;
  try {
    const raw = await fs.readFile(sessionPath, "utf-8");
    const entries = parseSessionEntries(raw);
    return extractLatestTodoSnapshotFromEntries(entries);
  } catch {
    return null;
  }
}
