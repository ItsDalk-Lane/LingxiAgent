/**
 * Context usage breakdown accounting at the final request boundary.
 *
 * The session coordinator wraps every AgentSession's streamFunction
 * (_installCachePrefixGuard) and hands the exact `{systemPrompt, messages,
 * tools}` that is about to reach the provider to `computeContextUsageEstimate`.
 * Classification therefore describes what actually entered the model context,
 * not what the renderer guesses — the ring's detail view never re-estimates
 * token counts in the UI layer.
 *
 * Token 口径:messages 用 pi SDK 的 estimateTokens（chars/4、block 感知）;
 * systemPrompt 分段与工具 schema 用同一个 chars/4 口径
 * (lib/llm/estimate-text-tokens.ts)。两侧同源,breakdown 与
 * getContextUsage().tokens 天然近似闭合,残余差值由读取侧的
 * reconcileContextUsageBreakdown 归入 other。
 */
import { estimateTokens } from "../pi-sdk/index.ts";
import { estimateTextTokens } from "./estimate-text-tokens.ts";
import type {
  ContextUsageBreakdown,
  ContextUsageBreakdownCategories,
  ContextUsageBreakdownEstimate,
} from "../../shared/context-usage-breakdown.ts";

/**
 * pi SDK buildSystemPrompt 的固定拼接标记:
 *   …<project_context>…</project_context>\n
 *   …formatSkillsForPrompt → "The following skills provide…" … <available_skills>…</available_skills>
 * 统计只认最终 systemPrompt 字符串里真实存在的闭合段,识别不了就留在 system,
 * 不按快照伪造分段。
 */
const PROJECT_CONTEXT_OPEN = "<project_context>";
const PROJECT_CONTEXT_CLOSE = "</project_context>";
const SKILLS_BLOCK_HEADER = "The following skills provide specialized instructions for specific tasks.";
const SKILLS_BLOCK_CLOSE = "</available_skills>";

/** MCP 工具(含 deferred 装配的 mcp_search_tools / mcp_describe_tool / mcp_call bridge)的统一前缀。 */
const MCP_TOOL_NAME_PREFIX = "mcp_";

function sliceMarkedSegment(text: string, startMarker: string, closeMarker: string, fromIndex = 0) {
  const start = text.indexOf(startMarker, fromIndex);
  if (start < 0) return null;
  const close = text.indexOf(closeMarker, start + startMarker.length);
  if (close < 0) return null;
  const end = close + closeMarker.length;
  return { start, end, segment: text.slice(start, end) };
}

/**
 * 把最终 systemPrompt 拆成 skills listing 段、project_context 段与主体。
 * 只处理第一处闭合段(SDK 每个标记只拼一次);标记不成对时原样归入主体。
 */
function splitSystemPromptSegments(systemPrompt: string) {
  let rest = systemPrompt;
  let files = "";
  let skills = "";

  const projectContext = sliceMarkedSegment(rest, PROJECT_CONTEXT_OPEN, PROJECT_CONTEXT_CLOSE);
  if (projectContext) {
    files = projectContext.segment;
    rest = rest.slice(0, projectContext.start) + rest.slice(projectContext.end);
  }

  const skillsClose = rest.indexOf(SKILLS_BLOCK_CLOSE);
  if (skillsClose >= 0) {
    const headerStart = rest.lastIndexOf(SKILLS_BLOCK_HEADER, skillsClose);
    if (headerStart >= 0) {
      const end = skillsClose + SKILLS_BLOCK_CLOSE.length;
      skills = rest.slice(headerStart, end);
      rest = rest.slice(0, headerStart) + rest.slice(end);
    }
  }

  return { rest, skills, files };
}

/** 单个进入请求体的工具 schema 估值:name + description + parameters,与消息同一 chars/4 口径。 */
function estimateToolSchemaTokens(tool: any): number {
  const name = typeof tool?.name === "string" ? tool.name : "";
  const description = typeof tool?.description === "string" ? tool.description : "";
  const parameters = tool?.parameters === undefined ? "" : JSON.stringify(tool.parameters);
  return estimateTextTokens(`${name}\n${description}\n${parameters}`);
}

function isMcpTool(tool: any): boolean {
  return typeof tool?.name === "string" && tool.name.startsWith(MCP_TOOL_NAME_PREFIX);
}

/**
 * 对一次最终请求做来源分类统计。context 即 pi StreamFunction 收到的
 * `{systemPrompt?, messages, tools?}`;deferred 工具不在 tools 里,不计入。
 */
export function computeContextUsageEstimate(context: any): ContextUsageBreakdownEstimate {
  const categories: ContextUsageBreakdownCategories = {
    system: 0,
    skills: 0,
    files: 0,
    tools: 0,
    mcp: 0,
    conversation: 0,
    user: 0,
    toolResults: 0,
  };

  if (typeof context?.systemPrompt === "string" && context.systemPrompt.length > 0) {
    const segments = splitSystemPromptSegments(context.systemPrompt);
    categories.system = estimateTextTokens(segments.rest);
    categories.skills = estimateTextTokens(segments.skills);
    categories.files = estimateTextTokens(segments.files);
  }

  const tools = Array.isArray(context?.tools) ? context.tools : [];
  for (const tool of tools) {
    if (!tool) continue;
    const tokens = estimateToolSchemaTokens(tool);
    if (isMcpTool(tool)) categories.mcp += tokens;
    else categories.tools += tokens;
  }

  const messages = Array.isArray(context?.messages) ? context.messages : [];
  let lastUserIndex = -1;
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i]?.role === "user") {
      lastUserIndex = i;
      break;
    }
  }
  for (let i = 0; i < messages.length; i++) {
    const message = messages[i];
    if (!message) continue;
    const tokens = estimateTokens(message);
    if (message.role === "toolResult") categories.toolResults += tokens;
    else if (message.role === "user" && i === lastUserIndex) categories.user += tokens;
    else categories.conversation += tokens;
  }

  return { ...categories, computedAt: Date.now() };
}

/**
 * 读取侧对账:other = total − 已分类合计(协议包装、provider tokenizer 与
 * chars/4 的口径差等无法单独拆分的部分),保证 sum(categories) + other 与
 * getContextUsage().tokens 闭合。total 未知(compaction 后)或无缓存估值时
 * 返回 null —— 显式无数据,不伪造明细。
 */
export function reconcileContextUsageBreakdown(
  estimate: ContextUsageBreakdownEstimate | null | undefined,
  totalTokens: number | null | undefined,
): ContextUsageBreakdown | null {
  if (!estimate || typeof totalTokens !== "number" || !Number.isFinite(totalTokens)) return null;
  const total = Math.max(0, Math.round(totalTokens));
  const known = estimate.system + estimate.skills + estimate.files + estimate.tools
    + estimate.mcp + estimate.conversation + estimate.user + estimate.toolResults;
  return {
    system: estimate.system,
    skills: estimate.skills,
    files: estimate.files,
    tools: estimate.tools,
    mcp: estimate.mcp,
    conversation: estimate.conversation,
    user: estimate.user,
    toolResults: estimate.toolResults,
    other: Math.max(0, total - known),
    total,
    computedAt: estimate.computedAt,
  };
}

const ESTIMATE_CATEGORY_KEYS: ReadonlyArray<keyof ContextUsageBreakdownCategories> = [
  "system", "skills", "files", "tools", "mcp", "conversation", "user", "toolResults",
];

/**
 * 校验 session-meta 落盘的 estimate 形态（entry 重建后的恢复来源）。
 * 任一分类缺失/非有限非负数、或整体全零无意义 → null，恢复侧不落库。
 * 落盘数据可能是旧版本/手改文件，不能带着畸形值进 reconcile。
 */
export function sanitizeContextUsageEstimate(value: unknown): ContextUsageBreakdownEstimate | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const source = value as Record<string, unknown>;
  const out: Record<string, number> = {};
  let hasNonZero = false;
  for (const key of ESTIMATE_CATEGORY_KEYS) {
    const raw = source[key];
    if (typeof raw !== "number" || !Number.isFinite(raw) || raw < 0) return null;
    out[key] = raw;
    if (raw > 0) hasNonZero = true;
  }
  if (!hasNonZero) return null;
  const computedAt = source.computedAt;
  out.computedAt = typeof computedAt === "number" && Number.isFinite(computedAt) ? computedAt : 0;
  return out as unknown as ContextUsageBreakdownEstimate;
}
