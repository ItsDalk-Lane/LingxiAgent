import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { parsePatch } from 'diff';
import { languages } from '@codemirror/language-data';
import { classHighlighter, highlightTree } from '@lezer/highlight';
import type { LanguageSupport } from '@codemirror/language';
import styles from './MessageActivity.module.css';

export function activityText(key: string, vars?: Record<string, string | number>): string {
  return (window.t ?? ((value: string) => value))(`messageActivity.${key}`, vars);
}

export function ActivityIcon({ kind }: { kind: string }) {
  let drawing: ReactNode;
  if (kind === 'thinking') drawing = <><ellipse cx="12" cy="12" rx="10" ry="4" transform="rotate(45 12 12)" /><ellipse cx="12" cy="12" rx="10" ry="4" transform="rotate(-45 12 12)" /><circle cx="12" cy="12" r="1" /></>;
  else if (['exec_command', 'bash', 'terminal', 'write_stdin'].includes(kind)) drawing = <><rect x="3" y="3" width="18" height="18" rx="4" /><path d="m7 8 3 3-3 3m6 2h4" /></>;
  else if (/search|grep|find/.test(kind)) drawing = <><circle cx="10.5" cy="10.5" r="7.5" /><path d="m16 16 5 5" /></>;
  else if (/edit|write/.test(kind)) drawing = <><path d="m15 3 6 6-12 12H3v-6Zm-2 2 6 6M3 21h18" /></>;
  else if (/read|skill/.test(kind)) drawing = <><rect x="5" y="2" width="14" height="20" rx="2" /><path d="M8 7h8M8 11h6" /></>;
  else if (kind === 'ls') drawing = <path d="M2 6h8l2 3h10v12H2Zm0 0V3h8l2 3h8v3" />;
  else drawing = <><rect x="3" y="3" width="7" height="7" rx="2" /><rect x="14" y="3" width="7" height="7" rx="2" /><rect x="3" y="14" width="7" height="7" rx="2" /><rect x="14" y="14" width="7" height="7" rx="2" /></>;
  return <svg className={styles.icon} data-activity-icon={kind} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{drawing}</svg>;
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
