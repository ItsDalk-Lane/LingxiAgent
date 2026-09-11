/**
 * todo-constants.js — 共享的 todo tool 相关常量
 *
 * 作为前后端筛选 todo 相关 tool_result 的单一来源。
 * 前端镜像在 desktop/src/react/utils/todo-constants.ts，必须保持同步。
 *
 * 命名不对称说明：tool 名用 snake_case（todo_write），对齐 Hana built-in
 * (read/write/edit/exec_command) 和其他项目 tool (recall_experience/update_settings) 的
 * wire format；i18n key 用 camelCase (toolDef.todoWrite)，对齐 JS 属性命名
 * 惯例。两种风格各自领域内一致，跨领域的不对称是故意的。
 */

/** 新 tool 正式名（对标 Claude Code TodoWrite） */
export const TODO_WRITE_TOOL_NAME = "todo_write";

/** 所有被识别为 todo 相关的 tool 名字（包括旧版以兼容历史 session） */
export const TODO_TOOL_NAMES = Object.freeze(["todo", TODO_WRITE_TOOL_NAME]);

/** Hana 内部 session 事件：用户手动完成并移除当前 todo group */
export const TODO_STATE_CUSTOM_TYPE = "lingxi.todo_state";

/**
 * 清单数据格式版本。details.todoVersion === TODO_FORMAT_VERSION 的记录
 * 采用新版生命周期：全部终态（完成/取消）保留收尾摘要，直到用户收纳或
 * 新一轮请求被正式接受；受阻/取消是合法状态。
 * 无版本标识的旧记录保持旧语义：全部 completed 即移除，不重新弹出。
 */
export const TODO_FORMAT_VERSION = 2;

/** 全部合法 todo 状态（含 v2 新增的 blocked / cancelled） */
export const TODO_STATUSES = Object.freeze([
  "pending",
  "in_progress",
  "blocked",
  "cancelled",
  "completed",
]);

/** 终态集合：完成或取消都算"已结束"，不计入剩余工作 */
export const TODO_TERMINAL_STATUSES = Object.freeze(["cancelled", "completed"]);
