/**
 * navigation.js — 记忆检索导航节（借鉴 nuphus L1 三段式注入的「标题+ID 引导自查」）
 *
 * 产出 memory/navigation.md，由 compile.assemble() 作为 memory.md 的第 5 段拼进
 * system prompt（cache 分界线后、按会话冻结）。目的是把记忆召回从「只推全文」
 * 变成「推索引 + 拉详情」：模型能看到最近会话的标题/ID 和事实库标签概览，
 * 知道还能用 search_memory / session 工具查更多。
 *
 * 约束（对齐 assemble 的同步纯文件设计）：
 *   - 本文件负责异步收集数据（listSessions 是 async）并落盘 navigation.md；
 *     assemble 只同步读文件，REST 编辑/dream/import 重拼 memory.md 时导航节不丢。
 *   - 数据缺失/读取失败只 log 不抛——导航是增益信息，不能阻塞记忆编译主链。
 */

import fs from "fs";
import path from "path";
import { getLocale } from "../i18n.ts";
import { atomicWriteSync } from "../../shared/safe-fs.ts";
import { createModuleLogger } from "../debug-log.ts";

const log = createModuleLogger("memory-navigation");

/** 最近会话条数上限 */
export const NAVIGATION_MAX_SESSIONS = 5;
/** 标签条数上限 */
export const NAVIGATION_MAX_TAGS = 10;
/** 会话标题回退到首条消息时的截断长度 */
export const NAVIGATION_TITLE_MAX_CHARS = 30;
/** 整节硬上限（防漂移；超限丢弃尾部标签行） */
export const NAVIGATION_MAX_CHARS = 500;

/**
 * 导航节正文（不含 `##` 标题——标题由 buildCompiledMemoryMarkdown 统一加）。
 * 纯函数；无任何数据时返回空串（assemble 会跳过该段）。
 *
 * @param {{ recentSessions?: Array<{ title?: string|null, sessionId?: string|null }>, tagCounts?: Array<{ tag: string, count: number }>, isZh?: boolean }} input
 * @returns {string}
 */
export function buildNavigationSection({
  recentSessions = [],
  tagCounts = [],
  isZh = true,
} = {}) {
  const sessions = (Array.isArray(recentSessions) ? recentSessions : [])
    .slice(0, NAVIGATION_MAX_SESSIONS)
    .map((s) => ({
      title: String(s?.title || "").trim(),
      sessionId: String(s?.sessionId || "").trim(),
    }))
    .filter((s) => s.title || s.sessionId);
  const tags = (Array.isArray(tagCounts) ? tagCounts : [])
    .slice(0, NAVIGATION_MAX_TAGS)
    .filter((t) => t && typeof t.tag === "string" && t.tag.trim());

  if (sessions.length === 0 && tags.length === 0) return "";

  const lines: string[] = [];
  if (sessions.length > 0) {
    lines.push(isZh ? "最近会话（用 session 工具按 sessionId 可读取详情）：" : "Recent sessions (use the session tool with sessionId to read details):");
    for (const s of sessions) {
      const title = (s.title || (isZh ? "（无标题）" : "(untitled)")).slice(0, NAVIGATION_TITLE_MAX_CHARS);
      const id = s.sessionId ? s.sessionId.slice(0, 8) : "";
      lines.push(id ? `- ${title}（${id}）` : `- ${title}`);
    }
  }
  if (tags.length > 0) {
    lines.push("");
    lines.push(isZh ? "事实库标签（search_memory 可按 tags 过滤）：" : "Fact-store tags (search_memory accepts tags filtering):");
    lines.push(tags.map((t) => `${t.tag}(${t.count})`).join(isZh ? "、" : ", "));
  }
  lines.push("");
  lines.push(isZh
    ? "以上未注入的记忆可用工具检索：search_memory 查事实库，session read 读历史会话。"
    : "Memories not injected above can be retrieved via tools: search_memory for the fact store, session read for past sessions.");

  let body = lines.join("\n");
  if (body.length > NAVIGATION_MAX_CHARS) {
    body = body.slice(0, NAVIGATION_MAX_CHARS).trimEnd() + (isZh ? "…" : "...");
  }
  return body;
}

/**
 * 收集数据并落盘 navigation.md。fire-and-forget 友好：永不抛错。
 *
 * @param {{ listSessions?: () => Promise<Array<any>>, agentId?: string, factStore?: { tagCounts?: () => Array<{ tag: string, count: number }> }, navigationPath: string }} deps
 * @returns {Promise<boolean>} 是否成功写入
 */
export async function refreshNavigationFile({
  listSessions,
  agentId,
  factStore,
  navigationPath,
}: {
  listSessions?: () => Promise<Array<any>>;
  agentId?: string;
  factStore?: { tagCounts?: () => Array<{ tag: string, count: number }> };
  navigationPath: string;
} = {} as any) {
  try {
    let sessions: any[] = [];
    if (typeof listSessions === "function") {
      sessions = await listSessions() || [];
    }
    const recentSessions = sessions
      .filter((s) => (
        (!agentId || s?.agentId === agentId)
        && !s?.agentDeleted
        && (s?.title || s?.firstMessage)
      ))
      .slice(0, NAVIGATION_MAX_SESSIONS)
      .map((s) => ({ title: s.title || s.firstMessage, sessionId: s.sessionId }));

    const tagCounts = factStore?.tagCounts?.() || [];
    const section = buildNavigationSection({
      recentSessions,
      tagCounts,
      isZh: getLocale().startsWith("zh"),
    });

    fs.mkdirSync(path.dirname(navigationPath), { recursive: true });
    atomicWriteSync(navigationPath, section);
    return true;
  } catch (err: any) {
    log.warn(`navigation refresh failed (non-blocking): ${err?.message || err}`);
    return false;
  }
}
