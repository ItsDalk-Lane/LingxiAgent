/**
 * todo-context-reminder — 轮中节流提醒（融合方案机制 3）。
 *
 * 借鉴 openclaude 的 todo_reminder attachment：模型长时间不更新清单时，
 * 把 harness 侧的权威清单状态重新注入上下文，防止清单与现实漂移
 * （"Do not let the plan go stale"）。
 *
 * 与 openclaude 的两处差异：
 * 1. 数据源是持久化快照（extractLatestTodoSnapshot，与历史恢复/界面同一
 *    提取函数），不是内存 appState——跨中断、跨重开依然正确。
 * 2. 注入点是 pi SDK 的 transformContext（每次模型调用前、convertToLlm
 *    之前），输出只进本次请求，不落盘、不污染历史、不占用持久上下文。
 *    openclaude 的 reminder 是落盘 attachment；我们不需要那层持久性，
 *    因为权威状态本身已在历史里。
 *
 * 节流（沿用 openclaude 参数）：距上次 todo_write ≥ 10 个工具调用，
 * 且距上次注入 ≥ 10 个工具调用。仅当快照存在且有未完成项时注入
 * （清单为空/已结束不打扰；推动"主动建清单"由工具 description 负责）。
 */

import { extractLatestTodoSnapshot } from "../tools/todo-compat.ts";

const TOOL_CALLS_SINCE_WRITE = 10;
const TOOL_CALLS_BETWEEN_REMINDERS = 10;

const REMINDER_OPEN = "<system-reminder>";
const REMINDER_CLOSE = "</system-reminder>";

/** 每个 agent 一份节流状态（闭包内，不随上下文持久）。 */
const installedAgents = new WeakSet<object>();

function countToolCallsSince(messages: any[], predicate: (m: any) => boolean): number {
  let count = 0;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role === "toolResult") {
      if (predicate(message)) return count;
      count += 1;
    }
  }
  return count;
}

function isTodoWriteResult(message: any): boolean {
  return message?.role === "toolResult" && message?.toolName === "todo_write";
}

/**
 * 计算当前应注入的提醒文本；不满足条件返回 null。
 * 导出以便测试。
 */
export function buildTodoContextReminder(messages: any[], state: { toolCallCountAtLastReminder: number }) {
  if (!Array.isArray(messages) || messages.length === 0) return null;

  const toolCallCount = messages.filter((m) => m?.role === "toolResult").length;
  const sinceWrite = countToolCallsSince(messages, isTodoWriteResult);
  if (sinceWrite < TOOL_CALLS_SINCE_WRITE) return null;
  if (toolCallCount - state.toolCallCountAtLastReminder < TOOL_CALLS_BETWEEN_REMINDERS) return null;

  let snapshot: any = null;
  try {
    snapshot = extractLatestTodoSnapshot(messages as any);
  } catch {
    return null;
  }
  if (!snapshot || snapshot.removed || snapshot.dismissed) return null;
  const todos = Array.isArray(snapshot.todos) ? snapshot.todos : [];
  const unfinished = todos.filter((td: any) => td.status !== "completed" && td.status !== "cancelled");
  if (unfinished.length === 0) return null;

  const lines = todos.map((td: any, i: number) => {
    const reason = td.status === "blocked" && td.blockedReason ? ` (blocked: ${td.blockedReason})` : "";
    return `${i + 1}. [${td.status}] ${td.content}${reason}`;
  });

  state.toolCallCountAtLastReminder = toolCallCount;
  return [
    `${REMINDER_OPEN}The todo list hasn't been updated recently. Current authoritative state (snapshot ${snapshot.version}):`,
    ...lines,
    `If this list has become stale, rewrite it to match reality; otherwise continue working from it. Never mention this reminder to the user.${REMINDER_CLOSE}`,
  ].join("\n");
}

/**
 * 在会话 agent 上安装轮中提醒：链在既有 transformContext 之后，
 * 命中节流条件时在上下文末尾追加一条 ephemeral user 消息。
 */
export function installTodoContextReminder(session: any) {
  const agent = session?.agent;
  if (!agent || installedAgents.has(agent)) return;
  installedAgents.add(agent);

  const state = { toolCallCountAtLastReminder: -TOOL_CALLS_BETWEEN_REMINDERS };
  const previous = agent.transformContext;
  agent.transformContext = async (messages: unknown, signal?: AbortSignal) => {
    const transformed = typeof previous === "function" ? await previous(messages, signal) : messages;
    try {
      const reminder = buildTodoContextReminder(transformed as any[], state);
      if (!reminder) return transformed;
      return [
        ...(Array.isArray(transformed) ? transformed : []),
        { role: "user", content: [{ type: "text", text: reminder }], timestamp: Date.now() },
      ];
    } catch {
      // 提醒失败不阻断主请求
      return transformed;
    }
  };
}
