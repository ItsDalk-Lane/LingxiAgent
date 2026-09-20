/**
 * BrowserCard — 浏览器浮动卡片
 *
 * 当前 session 浏览器运行时，在聊天区顶部显示浮动卡片。
 * 由 App.tsx 在 .main-content 内直接渲染。
 *
 * 跨会话接力：浏览器数据仍按 session 一对一隔离，但显示层共享——
 * 当前会话没有在跑的浏览器时，接力显示「最近活跃」的其他会话浏览器，
 * 点击卡片即打开那个会话的浏览器窗口。
 */

import { useCallback, useMemo } from 'react';
import { useStore } from '../stores';
import {
  setBrowserCardCollapsed,
  useBrowserState,
  type BrowserSessionState,
} from '../stores/browser-slice';
import { normalizeSessionPath } from '../stores/session-slice';

interface DisplayBrowser {
  /** 浏览器归属的会话路径（可能不是当前会话） */
  sessionPath: string | null;
  state: BrowserSessionState;
}

export function BrowserCard() {
  const currentPath = useStore(s => s.currentSessionPath);
  const currentState = useBrowserState();
  const browserBySession = useStore(s => s.browserBySession);

  // 当前会话自己的浏览器优先；没有的话，挑最近活跃的其他会话浏览器接力显示。
  const display: DisplayBrowser | null = useMemo(() => {
    if (currentPath && currentState.running) {
      return { sessionPath: currentPath, state: currentState };
    }
    const currentNorm = normalizeSessionPath(currentPath);
    let best: (DisplayBrowser & { activity: number }) | null = null;
    for (const value of Object.values(browserBySession || {})) {
      if (!value?.running) continue;
      const sp = value.sessionPath || null;
      if (sp && currentNorm && normalizeSessionPath(sp) === currentNorm) continue;
      const activity = value.lastActiveAt ?? value.thumbnailCapturedAt ?? 0;
      if (!best || activity > best.activity) best = { sessionPath: sp, state: value, activity };
    }
    return best;
  }, [currentPath, currentState, browserBySession]);

  const handleClick = useCallback(() => {
    // 接力显示的卡片打开归属会话的浏览器；归属不明时退回当前会话（旧行为）
    const sessionPath = display?.sessionPath ?? useStore.getState().currentSessionPath;
    window.platform?.openBrowserViewer?.(sessionPath ? { sessionPath } : undefined);
  }, [display]);

  // 叉只收起卡片，不碰浏览器本身：agent 的操作不能被一次视觉整理打断。
  // 真正的急停留在 viewer 工具栏。
  const handleCollapse = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    // 收起作用于归属会话：回到那个会话时卡片也保持收起（收起是用户意图）
    const sessionPath = display?.sessionPath ?? useStore.getState().currentSessionPath;
    if (sessionPath) setBrowserCardCollapsed(sessionPath, true);
  }, [display]);

  if (!display || !display.state.running || display.state.collapsed) return null;

  const { url: browserUrl, thumbnail: browserThumbnail } = display.state;

  let displayUrl = '';
  try {
    if (browserUrl) displayUrl = new URL(browserUrl).hostname;
  } catch {
    displayUrl = browserUrl || '';
  }

  return (
    <div className="browser-floating-card" id="browserFloatingCard" onClick={handleClick}>
      <div className="browser-floating-info">
        <div className="browser-floating-icon">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="10"></circle>
            <line x1="2" y1="12" x2="22" y2="12"></line>
            <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"></path>
          </svg>
        </div>
        <div className="browser-floating-text">
          <div className="browser-floating-label">{(window.t ?? ((p: string) => p))('browser.using')}</div>
          {displayUrl && (
            <div className="browser-floating-url">{displayUrl}</div>
          )}
        </div>
      </div>
      <div className="browser-floating-right">
        {browserThumbnail && (
          <img
            className="browser-floating-thumb"
            src={`data:image/jpeg;base64,${browserThumbnail}`}
            alt=""
            draggable={false}
          />
        )}
        <button className="browser-floating-close" title={(window.t ?? ((p: string) => p))('browser.collapse')} onClick={handleCollapse}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M18 6L6 18M6 6l12 12"></path>
          </svg>
        </button>
      </div>
    </div>
  );
}
