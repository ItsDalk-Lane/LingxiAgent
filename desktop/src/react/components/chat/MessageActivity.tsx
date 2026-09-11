import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { parsePatch } from 'diff';
import { languages } from '@codemirror/language-data';
import { classHighlighter, highlightTree } from '@lezer/highlight';
import type { LanguageSupport } from '@codemirror/language';
import styles from './MessageActivity.module.css';

export function activityText(key: string, vars?: Record<string, string | number>): string {
  return (window.t ?? ((value: string) => value))(`messageActivity.${key}`, vars);
}

/**
 * 工具行的图标家族。
 *
 * 判定只认登记过的工具名，不再按子串猜：旧实现里 `/search|grep|find/` 会把
 * `mcp_deep-search` 这种不可预知的插件/MCP 工具名也算成"搜索"，家族图标就落到了
 * 猜出来的地方。表里查不到的一律落通用网格。
 *
 * 顺序也要紧：`/read|write/`、`/search/` 这类旧正则曾经抢走 knowledge_read、
 * channel_reply 这些名字，所以家族分支必须排在它们之前。
 */
const ACTIVITY_ICON_FAMILIES: Readonly<Record<string, string>> = {
  // 记忆族
  search_memory: 'memory', pin_memory: 'memory', unpin_memory: 'memory',
  recall_experience: 'memory', record_experience: 'memory', tenet_propose: 'memory',
  // 知识族
  knowledge_search: 'knowledge', knowledge_read: 'knowledge', knowledge_outline: 'knowledge',
  knowledge_grep: 'knowledge', knowledge_manage: 'knowledge', knowledge_think: 'knowledge',
  knowledge_read_part: 'knowledge', knowledge_supplement: 'knowledge',
  knowledge_answer: 'knowledge', knowledge_local_search: 'knowledge',
  knowledge_research_plan: 'knowledge', knowledge_research_round: 'knowledge',
  knowledge_research_worker: 'knowledge', knowledge_research_progress: 'knowledge',
  knowledge_research_review: 'knowledge', knowledge_research_synthesis: 'knowledge',
  // 频道族
  channel: 'channel', channel_read_context: 'channel', channel_reply: 'channel', channel_pass: 'channel',
  notify: 'notify',
  browser: 'browser',
  computer: 'computer',
  // 文件族
  file: 'file', materialize: 'file',
  automation: 'automation',
  subagent_reply: 'subagent', subagent_close: 'subagent',
  stop_task: 'stop',
  // 会话 / 状态 / 清单 / 循环 / 卡片 / 私信
  session: 'session', session_folders: 'folder',
  current_status: 'status', check_pending_tasks: 'checklist', loop_control: 'loop',
  create_artifact: 'card', dm: 'dm',
  // Pi SDK 沙盒工具
  read: 'read', write: 'edit', edit: 'edit',
  grep: 'search', find: 'search', ls: 'ls',
  bash: 'terminal', terminal: 'terminal', exec_command: 'terminal', write_stdin: 'terminal',
  // 网页工具沿用原来的搜索/读取家族
  web_search: 'search', web_fetch: 'read',
  // 形态名
  thinking: 'thinking', skill: 'skill',
};

export function activityIconFamily(kind: string): string {
  return ACTIVITY_ICON_FAMILIES[kind] ?? 'grid';
}

export function ActivityIcon({ kind }: { kind: string }) {
  const family = activityIconFamily(kind);
  let drawing: ReactNode;
  if (family === 'thinking') drawing = <><ellipse cx="12" cy="12" rx="10" ry="4" transform="rotate(45 12 12)" /><ellipse cx="12" cy="12" rx="10" ry="4" transform="rotate(-45 12 12)" /><circle cx="12" cy="12" r="1" /></>;
  else if (family === 'terminal') drawing = <><rect x="3" y="3" width="18" height="18" rx="4" /><path d="m7 8 3 3-3 3m6 2h4" /></>;
  else if (family === 'memory') drawing = <path d="M20 11.5a7.5 7.5 0 0 0-11-6.6A5.5 5.5 0 0 0 6.5 15H17a4 4 0 0 0 3-3.5Zm-14 3.5 1 5" />;
  else if (family === 'knowledge') drawing = <><path d="M12 6.5C10 4.9 7.7 4 5 4H3v14h2c2.7 0 5 .9 7 2.5" /><path d="M12 6.5C14 4.9 16.3 4 19 4h2v14h-2c-2.7 0-5 .9-7 2.5" /><path d="M12 6.5v14" /></>;
  else if (family === 'channel') drawing = <><path d="M3 10v4h3l6 4V6L6 10H3Z" /><path d="M16.5 8.5a5 5 0 0 1 0 7" /><path d="M19.5 5.5a9 9 0 0 1 0 13" /></>;
  else if (family === 'notify') drawing = <><path d="M6 16v-5a6 6 0 1 1 12 0v5l1.5 2.5H4.5L6 16Z" /><path d="M10 20.5a2.2 2.2 0 0 0 4 0" /></>;
  else if (family === 'browser') drawing = <><circle cx="12" cy="12" r="9" /><path d="M3 12h18" /><path d="M12 3a13.5 13.5 0 0 1 0 18a13.5 13.5 0 0 1 0-18Z" /></>;
  else if (family === 'computer') drawing = <><rect x="2.5" y="4" width="19" height="12.5" rx="2" /><path d="M8.5 20.5h7M12 16.5v4" /></>;
  else if (family === 'file') drawing = <><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Z" /><path d="M14 3v5h5M9 13h6M9 17h4" /></>;
  else if (family === 'automation') drawing = <><circle cx="12" cy="12" r="3.2" /><path d="M12 3v2.5M12 18.5V21M4.2 7.5l2.2 1.3M17.6 15.2l2.2 1.3M4.2 16.5l2.2-1.3M17.6 8.8l2.2-1.3" /></>;
  else if (family === 'subagent') drawing = <><rect x="4" y="7" width="16" height="12" rx="3" /><path d="M12 3v4M8.5 13h.01M15.5 13h.01M9.5 16.5h5" /></>;
  else if (family === 'stop') drawing = <rect x="5" y="5" width="14" height="14" rx="3" />;
  else if (family === 'session') drawing = <><path d="M20 12a7.5 7.5 0 0 1-11 6.6L4 20l1.4-4.2A7.5 7.5 0 1 1 20 12Z" /><path d="M8.5 11h7M8.5 14.5h4" /></>;
  else if (family === 'folder') drawing = <path d="M3 7a2 2 0 0 1 2-2h3.8l2 2.5H19a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />;
  else if (family === 'status') drawing = <><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" /></>;
  else if (family === 'checklist') drawing = <><rect x="4" y="4" width="16" height="16" rx="2.5" /><path d="m8 10.5 1.6 1.6L13 8.6M8 16h7" /></>;
  else if (family === 'loop') drawing = <><path d="M4 9a7.5 7.5 0 0 1 12.6-4.4L20 8" /><path d="M20 4.5V8h-3.5" /><path d="M20 15a7.5 7.5 0 0 1-12.6 4.4L4 16" /><path d="M4 19.5V16h3.5" /></>;
  else if (family === 'card') drawing = <><rect x="3" y="5" width="18" height="14" rx="2.5" /><path d="M3 9.5h18" /></>;
  else if (family === 'dm') drawing = <><path d="M4 5.5h16v11H12l-4.5 3.5v-3.5H4Z" /><path d="M8.5 11h7" /></>;
  else if (family === 'search') drawing = <><circle cx="10.5" cy="10.5" r="7.5" /><path d="m16 16 5 5" /></>;
  else if (family === 'edit') drawing = <><path d="m15 3 6 6-12 12H3v-6Zm-2 2 6 6M3 21h18" /></>;
  else if (family === 'read' || family === 'skill') drawing = <><rect x="5" y="2" width="14" height="20" rx="2" /><path d="M8 7h8M8 11h6" /></>;
  else if (family === 'ls') drawing = <path d="M2 6h8l2 3h10v12H2Zm0 0V3h8l2 3h8v3" />;
  else drawing = <><rect x="3" y="3" width="7" height="7" rx="2" /><rect x="14" y="3" width="7" height="7" rx="2" /><rect x="3" y="14" width="7" height="7" rx="2" /><rect x="14" y="14" width="7" height="7" rx="2" /></>;
  return <svg className={styles.icon} data-activity-icon={kind} data-activity-family={family} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{drawing}</svg>;
}

export function CopyActivity({ content, disabled = false, labelKey = 'copy' }: { content: string; disabled?: boolean; labelKey?: string }) {
  const [state, setState] = useState<'copy' | 'copied' | 'copyFailed'>('copy');
  return <button type="button" className={styles.action} disabled={disabled} onClick={() => {
    void navigator.clipboard.writeText(content).then(() => setState('copied'), () => setState('copyFailed'));
  }}>{activityText(state === 'copy' ? labelKey : state)}</button>;
}

/** 只对当前可见片段着色，避免大历史结果阻塞聊天首屏。 */
function ColoredCode({ content, language }: { content: string; language?: string }) {
  const [support, setSupport] = useState<LanguageSupport | null>(null);
  useEffect(() => {
    let active = true;
    const name = language?.toLowerCase();
    const description = name && languages.find(item => item.name.toLowerCase() === name || item.alias.includes(name) || item.extensions.includes(name));
    if (description) void description.load().then(value => { if (active) setSupport(value); }, () => { if (active) setSupport(null); });
    else setSupport(null);
    return () => { active = false; };
  }, [language]);
  const spans = useMemo(() => {
    if (!support) return content;
    const children: ReactNode[] = [];
    let cursor = 0;
    highlightTree(support.language.parser.parse(content), classHighlighter, (from, to, classes) => {
      if (from > cursor) children.push(content.slice(cursor, from));
      children.push(<span className={classes} key={from}>{content.slice(from, to)}</span>);
      cursor = to;
    });
    children.push(content.slice(cursor));
    return children;
  }, [content, support]);
  return <span>{spans}</span>;
}

/** 完整补丁用解析器取真实修改行；截断补丁只移除文件头，保留已记录正文。 */
export function activityPatchLines(content: string): string[] {
  try {
    const patches = parsePatch(content);
    const lines = patches.flatMap(patch => patch.hunks.flatMap(hunk => hunk.lines));
    if (lines.length) return lines;
  } catch {
    // 历史预览可能在补丁中间截断，仍展示可用行，不补造内容。
  }
  let inHunk = false;
  return content.split('\n').filter(line => {
    if (/^diff --git /.test(line)) { inHunk = false; return false; }
    if (/^@@ /.test(line)) { inHunk = true; return false; }
    if (!inHunk && /^(?:--- |\+\+\+ |index |Index: |={4,})/.test(line)) return false;
    return true;
  });
}

export function ActivityLines({ content, startLine, language, diff = false, full = false, renderLine }: {
  content: string; startLine?: number; language?: string; diff?: boolean; full?: boolean; renderLine?: (line: string) => ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  const lines = diff ? activityPatchLines(content) : content.split('\n');
  const clipped = !full && !expanded && lines.length > 8;
  const indexes = clipped ? [0, 1, 2, 3, -1, lines.length - 4, lines.length - 3, lines.length - 2, lines.length - 1] : lines.map((_, i) => i);
  return <div className={styles.lines}>{indexes.map(i => i === -1
    ? <button className={styles.more} type="button" key="more" onClick={() => setExpanded(true)}>{activityText('remaining', { n: lines.length - 8 })}</button>
    : <div className={`${styles.line} ${diff && lines[i].startsWith('+') ? styles.added : diff && lines[i].startsWith('-') ? styles.removed : ''}`} key={i}>
      {startLine !== undefined && <span className={styles.number}>{startLine + i}</span>}
      {renderLine ? renderLine(lines[i]) : language && !diff ? <ColoredCode content={lines[i]} language={language} /> : <span>{lines[i] || ' '}</span>}
    </div>)}</div>;
}
