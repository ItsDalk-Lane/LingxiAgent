/**
 * use-session-search — 全局聊天搜索（标题 → 正文两阶段）
 *
 * 从 SessionList 内嵌搜索框抽离（任务十/十二）：同一 HTTP API
 * （/api/sessions/search?q=&phase=title|content&limit=20）、同样的
 * 180ms debounce + AbortController 竞态护栏，供居中搜索界面使用。
 * 不引入第二套搜索后端。
 */
import { useEffect, useMemo, useState } from 'react';
import { useStore } from '../stores';
import { lingxiFetch } from './use-hana-fetch';
import type { Session } from '../types';

export interface SessionSearchResult extends Session {
  matchKind: 'title' | 'content';
  snippet: string;
  score?: number;
}

export type SessionSearchStatus = 'idle' | 'title' | 'content' | 'done' | 'error';

export function normalizeSessionSearchResults(data: unknown): SessionSearchResult[] {
  if (!data || typeof data !== 'object' || Array.isArray(data)) return [];
  const results = (data as { results?: unknown }).results;
  if (!Array.isArray(results)) return [];

  return results.flatMap((raw): SessionSearchResult[] => {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return [];
    const item = raw as Partial<SessionSearchResult>;
    if (typeof item.path !== 'string' || !item.path) return [];
    return [{
      path: item.path,
      sessionId: typeof item.sessionId === 'string' ? item.sessionId : null,
      title: typeof item.title === 'string' ? item.title : null,
      firstMessage: typeof item.firstMessage === 'string' ? item.firstMessage : '',
      modified: typeof item.modified === 'string' ? item.modified : '',
      messageCount: typeof item.messageCount === 'number' ? item.messageCount : 0,
      agentId: typeof item.agentId === 'string' ? item.agentId : null,
      agentName: typeof item.agentName === 'string' ? item.agentName : null,
      cwd: typeof item.cwd === 'string' ? item.cwd : null,
      projectId: typeof item.projectId === 'string' ? item.projectId : null,
      pinnedAt: typeof item.pinnedAt === 'string' ? item.pinnedAt : null,
      pinOrder: typeof item.pinOrder === 'number' ? item.pinOrder : null,
      hasSummary: item.hasSummary === true,
      rcAttachment: null,
      agentDeleted: item.agentDeleted === true,
      readOnlyReason: typeof item.readOnlyReason === 'string' ? item.readOnlyReason : undefined,
      continuationAvailable: item.continuationAvailable === true,
      deletedAt: typeof item.deletedAt === 'string' ? item.deletedAt : undefined,
      matchKind: item.matchKind === 'content' ? 'content' : 'title',
      snippet: typeof item.snippet === 'string' ? item.snippet : '',
      score: typeof item.score === 'number' ? item.score : undefined,
    }];
  });
}

export function useSessionSearch(query: string) {
  const sessions = useStore(s => s.sessions);
  // 会话集变化（新建/重命名/新消息）时重跑当前查询，与原侧栏搜索行为一致。
  const sessionsSignature = useMemo(() => (
    sessions.map(s => `${s.path}:${s.title || ''}:${s.modified || ''}:${s.messageCount}:${s.projectId || ''}`).join('\n')
  ), [sessions]);

  const trimmedQuery = query.trim();
  const [titleResults, setTitleResults] = useState<SessionSearchResult[]>([]);
  const [contentResults, setContentResults] = useState<SessionSearchResult[]>([]);
  const [status, setStatus] = useState<SessionSearchStatus>('idle');

  useEffect(() => {
    if (!trimmedQuery) {
      setTitleResults([]);
      setContentResults([]);
      setStatus('idle');
      return;
    }

    let cancelled = false;
    const controller = new AbortController();
    setTitleResults([]);
    setContentResults([]);
    setStatus('title');

    const timer = window.setTimeout(async () => {
      const encodedQuery = encodeURIComponent(trimmedQuery);
      try {
        const titleRes = await lingxiFetch(`/api/sessions/search?q=${encodedQuery}&phase=title&limit=20`, {
          signal: controller.signal,
          timeout: 12_000,
        });
        const titleData = await titleRes.json();
        if (cancelled) return;
        setTitleResults(normalizeSessionSearchResults(titleData));
        setStatus('content');

        const contentRes = await lingxiFetch(`/api/sessions/search?q=${encodedQuery}&phase=content&limit=20`, {
          signal: controller.signal,
          timeout: 12_000,
        });
        const contentData = await contentRes.json();
        if (cancelled) return;
        setContentResults(normalizeSessionSearchResults(contentData));
        setStatus('done');
      } catch (err) {
        if (controller.signal.aborted || cancelled) return;
        console.warn('[session-search] failed:', err);
        setStatus('error');
      }
    }, 180);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [trimmedQuery, sessionsSignature]);

  return { titleResults, contentResults, status };
}
