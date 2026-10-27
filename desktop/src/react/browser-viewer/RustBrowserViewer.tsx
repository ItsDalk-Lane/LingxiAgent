import { useCallback, useEffect, useRef, useState } from 'react';
import type { RustBrowserAction, RustBrowserViewerContext, RustBrowserWorkspace } from '../types';
import { parseRustBrowserWorkspaces, rustBrowserRequest } from '../services/service-client';
import { usePlatform } from '../hooks/use-platform';

function tr(key: string, fallback: string): string { const value = window.t?.(key); return value && value !== key ? value : fallback; }
/** 这里展示实际服务状态；真正网页仍在该服务控制的同一 Chromium 窗口中。 */
export function RustBrowserViewer({ context }: { context: RustBrowserViewerContext }) {
  const [workspace, setWorkspace] = useState<RustBrowserWorkspace | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const alive = useRef(true);
  const currentContext = useRef(context);
  currentContext.current = context;
  const mutation = useRef<RustBrowserViewerContext | null>(null);
  const version = useRef(0);
  const refresh = useCallback(async () => {
    const requestVersion = version.current;
    const data = await rustBrowserRequest(context, 'workspaces');
    if (!alive.current || currentContext.current !== context || version.current !== requestVersion) return;
    const rows = parseRustBrowserWorkspaces(data, context);
    if (alive.current && currentContext.current === context) { setWorkspace(rows.find(row => row.sessionPath === context.sessionPath) || null); setError(''); }
  }, [context]);
  useEffect(() => {
    version.current += 1;
    alive.current = true;
    setWorkspace(null); setError(''); setBusy(false);
    let timer: ReturnType<typeof setTimeout> | undefined;
    let cancelled = false;
    const poll = async () => {
      try { if (mutation.current !== context) await refresh(); } catch (cause) { if (alive.current && currentContext.current === context) setError(cause instanceof Error ? cause.message : String(cause)); }
      if (!cancelled && alive.current && currentContext.current === context) timer = setTimeout(() => { void poll(); }, 2000);
    };
    void poll();
    return () => { cancelled = true; alive.current = false; clearTimeout(timer); };
  }, [refresh]);
  const command = async (action: RustBrowserAction | 'stop', tabId?: string) => {
    if (mutation.current === context) return;
    version.current += 1;
    mutation.current = context; setBusy(true);
    try {
      await rustBrowserRequest(context, action, tabId ? { tabId } : undefined);
      if (alive.current && currentContext.current === context) await refresh();
    } catch (cause) { if (alive.current && currentContext.current === context) setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { if (mutation.current === context) mutation.current = null; if (alive.current && currentContext.current === context) setBusy(false); }
  };
  const active = workspace?.tabs.find(tab => tab.tabId === workspace.activeTabId);
  return <main className="rust-browser-panel">
    <div className="rust-browser-drag-strip" aria-hidden="true" />
    <h2>{tr('browser.controlledWindow', '受控浏览器')}</h2>
    <p>{tr('browser.executionNode', '执行位置')}：{context.serviceLabel} · {context.executionNode}</p>
    <p style={{ overflowWrap: 'anywhere', opacity: .7 }}>{context.serviceUrl} · {context.sessionPath}</p>
    <p>{tr('browser.sameWindowHint', '网页在该服务的受控 Chromium 窗口中。显示、接管和恢复会操作同一页面。')}</p>
    {error && <p role="alert">{error}</p>}
    {workspace ? <>
      <p role="status">{workspace.userTakeover ? tr('browser.userTakeover', '你正在接管') : workspace.suspended ? tr('browser.suspended', '已暂停') : workspace.running ? tr('browser.running', '正在运行') : tr('browser.stopped', '已停止')}{workspace.revoked ? ` · ${tr('browser.revoked', '助手权限已撤销')}` : ''}</p>
      {workspace.unavailableReason && <p role="alert">{workspace.unavailableReason}</p>}
      <div className="rust-browser-controls">
        <button disabled={busy || !workspace.running || !workspace.visibleCapable} onClick={() => void command('show')}>{tr('browser.showControlled', '显示浏览器')}</button>
        <button disabled={busy || !workspace.running || !workspace.visibleCapable || workspace.userTakeover} onClick={() => void command('takeover')}>{tr('browser.takeover', '接管')}</button>
        <button disabled={busy || !workspace.running || !workspace.userTakeover} onClick={() => void command('release')}>{tr('browser.release', '交还助手')}</button>
        <button disabled={busy || !active?.canGoBack} onClick={() => void command('back')}>{tr('browser.back', '后退')}</button>
        <button disabled={busy || !active?.canGoForward} onClick={() => void command('forward')}>{tr('browser.forward', '前进')}</button>
        <button disabled={busy || !active} onClick={() => void command('reload')}>{tr('browser.reload', '刷新')}</button>
        <button disabled={busy || !workspace.running} onClick={() => void command('new_tab')}>{tr('browser.newTab', '新标签页')}</button>
        <button disabled={busy || (!workspace.running && !workspace.resumable)} onClick={() => void command('stop')}>{tr('browser.emergencyStop', '急停')}</button>
      </div>
      {!workspace.visibleCapable && <p>{tr('browser.noVisibleWindow', '该服务不支持显示网页窗口，无法在这里手动接管。')}</p>}
      <p style={{ overflowWrap: 'anywhere' }}>{workspace.title || workspace.url || ''}</p>
      {workspace.tabs.length === 0 && <p>{tr('browser.emptyWorkspace', '没有打开的标签页')}</p>}
      <ul className="rust-browser-tabs">{workspace.tabs.map(tab => <li key={tab.tabId}>
        <button disabled={busy || tab.tabId === workspace.activeTabId} onClick={() => void command('switch_tab', tab.tabId)}>{tab.title || tab.url || tr('browser.newTab', '新标签页')}</button>{' '}
        <button disabled={busy} onClick={() => void command('close_tab', tab.tabId)} aria-label={`${tr('browser.closeTab', '关闭标签页')} ${tab.title}`}>×</button>
      </li>)}</ul>
    </> : !error && <p role="status">{tr('browser.workspaceUnavailable', '此聊天没有可用的浏览器状态')}</p>}
    <button onClick={() => usePlatform()?.closeBrowserViewer?.()}>{tr('browser.closeBtn', '关闭')}</button>
  </main>;
}
