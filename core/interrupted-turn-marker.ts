/**
 * 中断标记（融合方案机制 4a，借鉴 Codex <turn_aborted>）。
 *
 * 用户停止本轮后，向会话历史追加一条模型可见的合成 user 消息，明确告知：
 * 上一轮被有意停止、被中断的工具可能只执行了一半、清单里标记 in_progress
 * 的项并未在运行；继续前先核实真实状态。持久化进历史，下一轮（即使跨
 * 重开/压缩）模型都能看到。措辞取 Codex 的核实指引，而非 openclaude 的
 * "STOP and wait"——目标是"中断后正确地继续"，不是单纯停下。
 *
 * 调用时机：SDK abort settle 之后（core/session-coordinator.ts
 * _forceReleaseStreamingSession），保证落在中止的 assistant 消息之后。
 */

export const INTERRUPTED_TURN_MARKER_PREFIX = "<hana-turn-interrupted>";

export const INTERRUPTED_TURN_MARKER_TEXT =
  `${INTERRUPTED_TURN_MARKER_PREFIX}The previous turn was stopped on purpose. ` +
  `Aborted tools and commands may have partially executed, and task-list items marked in_progress are not actually running. ` +
  `Before continuing, verify the real state of the affected work, then update the task list accordingly.</hana-turn-interrupted>`;

/** 开发期曾短暂使用的旧版明文前缀；去重与客户端过滤都兼容它。 */
const LEGACY_MARKER_PREFIX = "[Turn interrupted by user]";

function userTextOf(message: any): string {
  if (!message || message.role !== "user") return "";
  if (typeof message.content === "string") return message.content;
  if (!Array.isArray(message.content)) return "";
  return message.content.map((block: any) => block?.text || "").join("");
}

/**
 * 向会话历史追加中断标记。返回 true = 已写入；false = 跳过（历史末尾
 * 已是中断标记，连续停止/重复触发不重复写）。
 * 标记走 <hana-turn-interrupted> 系统消息惯例：模型可见，UI 按既有
 * 过滤规则（同 <hana-background-result>）不展示。
 */
export function appendInterruptedTurnMarker(sessionManager: any): boolean {
  if (!sessionManager) return false;
  const messages = sessionManager.buildSessionContext?.().messages || [];
  const last = messages[messages.length - 1];
  const lastText = userTextOf(last);
  if (lastText.startsWith(INTERRUPTED_TURN_MARKER_PREFIX) || lastText.startsWith(LEGACY_MARKER_PREFIX)) return false;
  sessionManager.appendMessage({
    role: "user",
    content: [{ type: "text", text: INTERRUPTED_TURN_MARKER_TEXT }],
    timestamp: Date.now(),
  });
  return true;
}
