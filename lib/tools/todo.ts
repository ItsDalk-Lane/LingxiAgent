/**
 * todo.js — todo_write tool
 *
 * 替换式协议、五态状态机、content/activeForm 双文本（v2 新增 blocked/cancelled
 * 与 blockedReason）。完全无状态：不持有闭包变量，每次调用从参数构建返回值。
 *
 * 校验规则（计划书 §6.3）：
 * - 每次提交完整清单；content/activeForm 去除首尾空白。
 * - 空白条目、重复条目明确拒绝（isError），不静默 sanitize。
 * - blocked 必须给出 blockedReason（缺少什么）。
 * - 多个 in_progress 是合法情况（工作实际并行），不再告警。
 * - 成功的结果携带 todoVersion = TODO_FORMAT_VERSION，驱动新版生命周期
 *   （收尾摘要 / 收纳 / 用户取消）；失败结果不携带 todos 快照字段，
 *   实时与历史恢复据此保留最后一份有效清单（A13）。
 *
 * 收尾验证提醒（融合方案机制 4b，借鉴 openclaude verificationNudge）：
 * 一次性把 3+ 项清单全部标为 completed 且清单中没有任何验证类条目时，
 * 在工具结果后追加提醒——在模型最容易跳过验证的精确时刻（收尾时）拦截。
 *
 * 历史状态重建由 lib/tools/todo-compat.ts 的 extractLatestTodos 负责，
 * session_coordinator / sessions.js route 从 session entries 里读取。
 */

import { Type, StringEnum } from "../pi-sdk/index.ts";
import { t } from "../i18n.ts";
import { TODO_FORMAT_VERSION, TODO_STATUSES, TODO_WRITE_TOOL_NAME } from "./todo-constants.ts";
import { createModuleLogger } from "../debug-log.ts";

const log = createModuleLogger("todo_write");

/**
 * 校验并规范化条目：trim 文本；返回 { todos } 或 { errors }。
 * 拒绝规则：空白条目、重复 content、blocked 缺少原因。
 */
function normalizeTodos(input) {
  const errors = [];
  const seen = new Set();
  const todos = [];
  const rawList = Array.isArray(input) ? input : [];
  rawList.forEach((raw, index) => {
    const content = typeof raw?.content === "string" ? raw.content.trim() : "";
    const activeForm = typeof raw?.activeForm === "string" ? raw.activeForm.trim() : "";
    if (!content || !activeForm) {
      errors.push(`todo[${index}]: content and activeForm must be non-empty after trimming`);
      return;
    }
    if (seen.has(content)) {
      errors.push(`todo[${index}]: duplicate content "${content}"`);
      return;
    }
    seen.add(content);
    const item: { content: string; activeForm: string; status: any; blockedReason?: string } = {
      content,
      activeForm,
      status: raw.status,
    };
    const blockedReason = typeof raw?.blockedReason === "string" ? raw.blockedReason.trim() : "";
    if (raw.status === "blocked") {
      if (!blockedReason) {
        errors.push(`todo[${index}]: blocked item "${content}" requires blockedReason (what is missing)`);
        return;
      }
      item.blockedReason = blockedReason;
    } else if (blockedReason) {
      item.blockedReason = blockedReason;
    }
    todos.push(item);
  });
  return errors.length ? { errors } : { todos };
}

/**
 * 校验并构建文本摘要
 */
function buildSummary(todos) {
  if (todos.length === 0) return t("toolDef.todoWrite.summaryEmpty");
  const counts = { pending: 0, in_progress: 0, blocked: 0, cancelled: 0, completed: 0 };
  for (const td of todos) counts[td.status] = (counts[td.status] || 0) + 1;
  return t("toolDef.todoWrite.summaryStats", {
    total: todos.length,
    completed: counts.completed,
    in_progress: counts.in_progress,
    pending: counts.pending,
    blocked: counts.blocked,
    cancelled: counts.cancelled,
  });
}

/**
 * 验证类条目的识别（收尾验证提醒用）：content/activeForm 含验证语义词汇。
 */
const VERIFICATION_STEP_PATTERN = /verif|测试|test|check|验证|检查|核对|review|评审/i;

/**
 * 是否需要在结果后追加收尾验证提醒：3+ 项全部 completed 且没有验证类条目。
 */
function needsVerificationNudge(todos) {
  if (!Array.isArray(todos) || todos.length < 3) return false;
  if (!todos.every((td) => td.status === "completed")) return false;
  return !todos.some(
    (td) => VERIFICATION_STEP_PATTERN.test(td.content) || VERIFICATION_STEP_PATTERN.test(td.activeForm),
  );
}

const VERIFICATION_NUDGE_TEXT =
  "\n\nNote: you closed out a 3+ item list that contained no verification step. Before ending the turn, confirm every item is genuinely done and verified; if verification work is missing, do it now or honestly move the affected item back out of completed.";

/**
 * 创建 todo_write 工具定义
 * @returns {import('../pi-sdk/index.ts').ToolDefinition}
 */
export function createTodoTool() {
  return {
    name: TODO_WRITE_TOOL_NAME,
    label: "Todo",
    description: "Manage the session todo list for multi-step work. Decompose complex tasks into sub-tasks; skip it for simple single-step tasks. Each call replaces the entire list. Statuses: pending / in_progress / blocked / cancelled / completed. Multiple in_progress items are allowed when work is genuinely parallel. Discipline: mark an item in_progress BEFORE starting it and completed IMMEDIATELY when it finishes — never batch completions after the fact; do not let the list drift from reality while working; before ending the turn every item should reach a terminal state (completed/cancelled) or stay honestly unfinished. Never mark blocked or cancelled items as completed; blocked requires blockedReason (what is missing); items cancelled by the user stay cancelled unless the user explicitly reopens the work. If the previous turn was interrupted, reconcile against the authoritative list state injected at the start of this turn before rewriting the plan — items marked in_progress may have only partially executed. Only mark an item completed when it is truly done and verified.",
    parameters: Type.Object({
      todos: Type.Array(
        Type.Object({
          content: Type.String({
            minLength: 1,
            description: "Static description of the todo",
          }),
          activeForm: Type.String({
            minLength: 1,
            description: "In-progress form description (shown in UI while in_progress)",
          }),
          status: StringEnum(TODO_STATUSES, {
            description: "One of: pending / in_progress / blocked / cancelled / completed",
          }),
          blockedReason: Type.Optional(
            Type.String({
              description: "Required when status is blocked: what is missing / why it cannot continue",
            }),
          ),
        }),
        { description: "Complete todo list; each call replaces the previous list" },
      ),
    }),
    sessionPermission: {
      resolveInvocation: () => ({
        action: "replace",
        kind: "routine",
        capability: "todo_write.replace",
      }),
    },

    execute: async (_toolCallId, params, _signal, _onUpdate, _ctx) => {
      const { todos, errors } = normalizeTodos(params.todos || []);
      if (errors) {
        // 明确拒绝：不产出 todos 快照字段，调用方保留最后一份有效清单。
        log.warn(`rejected invalid todo list: ${errors.join("; ")}`);
        return {
          content: [{ type: "text", text: `Invalid todo list: ${errors.join("; ")}. Resubmit the complete list with these problems fixed.` }],
          details: { error: "invalid_todos", reasons: errors },
          isError: true,
        };
      }

      const summary = buildSummary(todos);
      // 收尾验证提醒（机制 4b）：全部 completed 的 3+ 项清单且无验证类条目时追加。
      const text = needsVerificationNudge(todos) ? summary + VERIFICATION_NUDGE_TEXT : summary;
      return {
        content: [{ type: "text", text }],
        details: { todos, todoVersion: TODO_FORMAT_VERSION },
      };
    },
  };
}
