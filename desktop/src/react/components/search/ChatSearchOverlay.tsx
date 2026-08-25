/**
 * ChatSearchOverlay — 居中聊天搜索界面（任务十一/十二）
 *
 * Titlebar 放大镜打开；空查询显示全局聊天记录（store.sessions 全量，
 * 不经工作台过滤）；输入后复用 use-session-search 的两阶段搜索
 * （标题 → 正文）。点击结果沿用既有行为：标题命中 switchSession、
 * 内容命中 locateSearchHit；workspace 由 switchSession 内的
 * Session → Workspace 恢复机制跟随。不新增筛选/历史等额外能力。
 */
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useStore } from '../../stores';
import { useI18n } from '../../hooks/use-i18n';
import { useSessionSearch, type SessionSearchResult } from '../../hooks/use-session-search';
import { switchSession } from '../../stores/session-actions';
import { locateSearchHit } from '../../stores/chat-find-actions';
import { buildSessionSections } from '../session-sections';
import { formatSessionDate } from '../../utils/format';
import type { Agent, Session } from '../../types';
import { AgentAvatar, resolveAgentDisplayInfo } from '../../utils/agent-display';
import styles from './ChatSearchOverlay.module.css';

export const SessionSearchItem = memo(function SessionSearchItem({
  result,
  isActive,
  agents,
  query,
  onSelect,
}: {
  result: SessionSearchResult;
  isActive: boolean;
  agents: Agent[];
  query: string;
  onSelect: (result: SessionSearchResult) => void;
}) {
  const { t } = useI18n();
  const parts: string[] = [];
  if (result.agentDeleted === true) parts.push(t('session.deletedAgent.meta'));
  if (result.agentName || result.agentId) parts.push(result.agentName || result.agentId!);
  if (result.cwd) {
    const dirName = result.cwd.split(/[/\\]/).filter(Boolean).pop();
    if (dirName) parts.push(dirName);
  }
  if (result.modified) parts.push(formatSessionDate(result.modified));

  const handleClick = useCallback(() => {
    onSelect(result);
  }, [onSelect, result]);

  return (
    <button
      type="button"
      className={`${styles.searchItem}${isActive ? ` ${styles.searchItemActive}` : ''}`}
      data-session-path={result.path}
      onClick={handleClick}
    >
      <div className={styles.searchItemHeader}>
        {result.agentId && (
          <SearchAgentBadge agentId={result.agentId} agentName={result.agentName} agents={agents} />
        )}
        <div className={styles.searchItemTitle}>
          {result.title || result.firstMessage || t('session.untitled')}
        </div>
      </div>
      <div className={styles.searchItemMeta}>{parts.join(' · ')}</div>
      {result.snippet && (
        <div className={styles.searchSnippet}>{result.snippet}</div>
      )}
    </button>
  );
});

const SearchAgentBadge = memo(function SearchAgentBadge({ agentId, agentName, agents }: {
  agentId: string;
  agentName: string | null;
  agents: Agent[];
}) {
  const info = resolveAgentDisplayInfo({
    id: agentId,
    agents,
    fallbackAgentName: agentName || agentId,
  });

  return (
    <AgentAvatar
      info={info}
      className={styles.searchAgentBadge}
      title={agentName || agentId}
    />
  );
});

function sessionAsTitleResult(session: Session): SessionSearchResult {
  // 空查询的全局列表复用搜索结果行组件：按标题命中处理，点击只切会话。
  return { ...session, matchKind: 'title', snippet: '' };
}

export function ChatSearchOverlay() {
  const { t } = useI18n();
  const open = useStore(s => s.chatSearchOpen);
  const setChatSearchOpen = useStore(s => s.setChatSearchOpen);
  const sessions = useStore(s => s.sessions);
  const agents = useStore(s => s.agents);
  const currentSessionPath = useStore(s => s.currentSessionPath);
  const pendingSessionSwitchPath = useStore(s => s.pendingSessionSwitchPath);
  const pendingNewSession = useStore(s => s.pendingNewSession);

  const [query, setQuery] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);
  const { titleResults, contentResults, status } = useSessionSearch(open ? query : '');

  // 打开时聚焦输入框并清空上次的查询；关闭时同样清空，避免下次打开闪现旧结果。
  useEffect(() => {
    if (!open) return;
    setQuery('');
    const timer = window.setTimeout(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    }, 0);
    return () => window.clearTimeout(timer);
  }, [open]);

  const close = useCallback(() => {
    setChatSearchOpen(false);
  }, [setChatSearchOpen]);

  // Escape 关闭（焦点不在输入框上也生效）。
  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') close();
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [close, open]);

  const trimmedQuery = query.trim();
  const activeSessionPath = pendingSessionSwitchPath || currentSessionPath;

  const handleSelect = useCallback((result: SessionSearchResult) => {
    close();
    if (result.matchKind === 'content' && trimmedQuery) {
      void locateSearchHit(result.path, trimmedQuery);
      return;
    }
    switchSession(result.path);
  }, [close, trimmedQuery]);

  const globalSections = useMemo(
    () => buildSessionSections(sessions, { mode: 'time' }),
    [sessions],
  );
  const titleResultPaths = useMemo(() => new Set(titleResults.map(result => result.path)), [titleResults]);
  const visibleContentResults = useMemo(
    () => contentResults.filter(result => !titleResultPaths.has(result.path)),
    [contentResults, titleResultPaths],
  );
  const hasSearchResults = titleResults.length > 0 || visibleContentResults.length > 0;

  if (!open) return null;

  return createPortal(
    <div
      className={styles.backdrop}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <div
        className={styles.overlay}
        role="dialog"
        aria-modal="true"
        aria-label={t('sidebar.searchPlaceholder')}
        data-chat-search-overlay=""
      >
        <div className={styles.searchRow}>
          <svg className={styles.searchIcon} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="11" cy="11" r="8" />
            <path d="m21 21-4.3-4.3" />
          </svg>
          <input
            ref={inputRef}
            className={styles.searchInput}
            value={query}
            placeholder={t('sidebar.searchPlaceholder')}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Escape') close();
            }}
            data-chat-search-input=""
          />
          <button
            type="button"
            className={styles.closeButton}
            aria-label={t('common.close')}
            onClick={close}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M18 6 6 18" />
              <path d="m6 6 12 12" />
            </svg>
          </button>
        </div>
        <div className={styles.results}>
          {!trimmedQuery ? (
            sessions.length === 0 ? (
              <div className={styles.resultStatus}>{t('sidebar.empty')}</div>
            ) : globalSections.map(section => (
              <section key={section.id} className={styles.resultSection}>
                <div className={styles.resultSectionTitle}>{t(section.titleKey)}</div>
                {section.items.map(session => (
                  <SessionSearchItem
                    key={session.path}
                    result={sessionAsTitleResult(session)}
                    isActive={!pendingNewSession && session.path === activeSessionPath}
                    agents={agents}
                    query=""
                    onSelect={handleSelect}
                  />
                ))}
              </section>
            ))
          ) : status === 'error' ? (
            <div className={styles.resultStatus}>{t('sidebar.searchFailed')}</div>
          ) : (
            <>
              {titleResults.length > 0 && (
                <section className={styles.resultSection}>
                  <div className={styles.resultSectionTitle}>{t('sidebar.searchTitleMatches')}</div>
                  {titleResults.map(result => (
                    <SessionSearchItem
                      key={`title:${result.path}`}
                      result={result}
                      isActive={!pendingNewSession && result.path === activeSessionPath}
                      agents={agents}
                      query={trimmedQuery}
                      onSelect={handleSelect}
                    />
                  ))}
                </section>
              )}
              {status === 'title' && (
                <div className={styles.resultStatus}>{t('sidebar.searchingTitles')}</div>
              )}
              {(visibleContentResults.length > 0 || status === 'content') && (
                <section className={styles.resultSection}>
                  <div className={styles.resultSectionTitle}>{t('sidebar.searchContentMatches')}</div>
                  {status === 'content' && visibleContentResults.length === 0 ? (
                    <div className={styles.resultStatus}>{t('sidebar.searchingContent')}</div>
                  ) : visibleContentResults.map(result => (
                    <SessionSearchItem
                      key={`content:${result.path}`}
                      result={result}
                      isActive={!pendingNewSession && result.path === activeSessionPath}
                      agents={agents}
                      query={trimmedQuery}
                      onSelect={handleSelect}
                    />
                  ))}
                </section>
              )}
              {status === 'done' && !hasSearchResults && (
                <div className={styles.resultStatus}>{t('sidebar.searchNoResults')}</div>
              )}
            </>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}
