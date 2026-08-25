/**
 * Context usage breakdown — shared contract between the server-side accounting
 * (core/session-coordinator at the final streamFn boundary) and the desktop
 * ContextRing detail view.
 *
 * The categories describe the real sources of the final chat-model request:
 *   system       System Prompt 主体（平台/环境/记忆等,减去可识别的 skills/project_context 段）
 *   skills       systemPrompt 中的 skills listing 段（<available_skills>…</available_skills>）
 *   files        systemPrompt 中的 <project_context>…</project_context> 段
 *   tools        实际进入请求的非 MCP 工具 schema
 *   mcp          实际进入请求的 MCP 来源工具 schema（mcp_ 前缀,含 mcp bridge 工具）
 *   conversation 历史消息（user/assistant 交替及派生消息,除去当前输入与 toolResult）
 *   user         当前用户输入（最后一条 user message）
 *   toolResults  role=toolResult 的消息
 *   other        协议/tokenizer 无法单独拆分的开销 + 与总量闭合的差值
 *
 * Deferred 工具的 schema 不进入请求体,天然不在统计内。
 */

export const CONTEXT_USAGE_BREAKDOWN_CATEGORIES = [
  "system",
  "skills",
  "files",
  "tools",
  "mcp",
  "conversation",
  "user",
  "toolResults",
  "other",
] as const;

export type ContextUsageBreakdownCategory = (typeof CONTEXT_USAGE_BREAKDOWN_CATEGORIES)[number];

/** 可直接归类的来源（不含 other,other 是读取时与总量对账得出的差值）。 */
export interface ContextUsageBreakdownCategories {
  system: number;
  skills: number;
  files: number;
  tools: number;
  mcp: number;
  conversation: number;
  user: number;
  toolResults: number;
}

/** 请求边界（streamFn）缓存的形态：分类估值 + 统计时间。 */
export interface ContextUsageBreakdownEstimate extends ContextUsageBreakdownCategories {
  computedAt: number;
}

/**
 * WS context_usage / compaction_end 消息里的 breakdown 形态。
 * total 是闭合基准（最近一次 getContextUsage().tokens）；
 * compaction 后 tokens 未知时整个 breakdown 为 null,不伪造明细。
 */
export interface ContextUsageBreakdown extends ContextUsageBreakdownCategories {
  other: number;
  total: number;
  computedAt: number;
}
