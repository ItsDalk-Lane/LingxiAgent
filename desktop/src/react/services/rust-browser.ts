import { useStore } from '../stores';
import { captureConversationRequestScope } from './conversation-request-scope';
import { resolveServerConnection, type ServerConnection } from './server-connection';
import type { BrowserViewerOpenTarget, RustBrowserViewerContext, RustBrowserWorkspace, RustBrowserAction } from '../types';
import { usePlatform } from '../hooks/use-platform';

export function rustBrowserRoute(connection: { serverNodeKind?: string }, path: string): string {
  return connection.serverNodeKind === 'lingxi-service' && /^\/api\/(?:browser(?:[/?]|$)|preferences\/browser(?:[/?]|$)|search\/(?:verify|config)(?:[?]|$)|preview\/html(?:[?]|$))/.test(path)
    ? `/lingxi/v1${path.slice('/api'.length)}` : path;
}
export function currentRustBrowserConnection(): ServerConnection | null {
  const connection = resolveServerConnection(useStore.getState());
  return connection?.serverNodeKind === 'lingxi-service' ? connection : null;
}
let observerStarted = false;
let connectionVersion = 0;
let registeredIdentity: string | null = null;
let registration: Promise<string | null> | null = null;
function identity() { return JSON.stringify(resolveServerConnection(useStore.getState())); }
function syncConnection(): Promise<string | null> {
  const next = identity();
  if (next === registeredIdentity && registration) return registration;
  const changed = registeredIdentity !== null;
  registeredIdentity = next;
  connectionVersion += 1;
  if (changed) useStore.setState({ browserBySession: {} });
  const connection = currentRustBrowserConnection();
  const browserPlatform = usePlatform();
  registration = browserPlatform?.setBrowserConnection
    ? browserPlatform.setBrowserConnection(connection) : Promise.resolve(null);
  // 订阅回调不会留下未处理的拒绝；需要操作的调用方仍会收到原始错误。
  const pending = registration;
  void pending.catch(() => { if (registration === pending) registration = null; });
  return registration;
}
export function startRustBrowserConnectionObserver(): void {
  if (observerStarted || typeof useStore.subscribe !== 'function') return;
  observerStarted = true;
  useStore.subscribe(() => { if (identity() !== registeredIdentity) void syncConnection(); });
  void syncConnection();
}
export async function rustBrowserTarget(sessionPath: string): Promise<BrowserViewerOpenTarget> {
  if (!/^native:[^\s/?#\u0000-\u001f]{1,256}$/.test(sessionPath)) throw new Error('browser_native_session_required');
  if (!usePlatform()?.setBrowserConnection) throw new Error('browser_desktop_unavailable');
  startRustBrowserConnectionObserver();
  const connection = currentRustBrowserConnection();
  if (!connection) throw new Error('browser_connection_required');
  const promise = syncConnection();
  const version = connectionVersion;
  const connectionKey = await promise;
  if (version !== connectionVersion || !connectionKey) throw new Error('browser_connection_changed');
  return { sessionPath, connectionKey };
}
export async function openRustBrowserViewer(sessionPath: string, restore = false, url?: string): Promise<void> {
  if (url !== undefined && !/^https?:$/.test(new URL(url).protocol)) throw new Error('browser_url_invalid');
  const scope = captureConversationRequestScope();
  try {
    const preparing = rustBrowserTarget(sessionPath);
    const version = connectionVersion;
    const target = await preparing;
    if (version !== connectionVersion || !scope.isCurrent()) throw new Error('browser_connection_changed');
    if (!usePlatform()?.openBrowserViewer) throw new Error('browser_desktop_unavailable');
    const requestPlatform = usePlatform();
    if (url !== undefined) {
      if (!requestPlatform.rustBrowserRequest) throw new Error('browser_desktop_unavailable');
      await requestPlatform.rustBrowserRequest(target, 'new_tab', { url });
    } else if (restore) {
      if (!requestPlatform.rustBrowserRequest) throw new Error('browser_desktop_unavailable');
      await requestPlatform.rustBrowserRequest(target, 'open');
    }
    if (version !== connectionVersion || !scope.isCurrent()) throw new Error('browser_connection_changed');
    // main 再次校验句柄，旧请求即使成功也不能打开别的连接的窗口。
    await usePlatform().openBrowserViewer(target);
  } finally { scope.dispose(); }
}
export function parseRustBrowserWorkspaces(value: unknown, context: Pick<RustBrowserViewerContext, 'executionNode'>): RustBrowserWorkspace[] {
  if (!value || typeof value !== 'object' || !('workspaces' in value) || !Array.isArray(value.workspaces)) throw new Error('browser_response_invalid');
  const seen = new Set<string>();
  return value.workspaces.map((item: unknown) => {
    const row = item as RustBrowserWorkspace;
    if (!row || typeof row !== 'object' || row.executionNode !== context.executionNode || typeof row.sessionId !== 'string' || row.sessionPath !== `native:${row.sessionId}` || typeof row.running !== 'boolean' || typeof row.resumable !== 'boolean' || typeof row.visibleCapable !== 'boolean' || typeof row.suspended !== 'boolean' || typeof row.userTakeover !== 'boolean' || typeof row.revoked !== 'boolean' || ![row.url, row.title, row.activeTabId, row.unavailableReason].every(value => value === null || typeof value === 'string') || !Array.isArray(row.tabs) || row.tabs.some(tab => !tab || typeof tab.tabId !== 'string' || typeof tab.title !== 'string' || typeof tab.canGoBack !== 'boolean' || typeof tab.canGoForward !== 'boolean' || !(tab.url === null || typeof tab.url === 'string') || typeof tab.createdAt !== 'string' || !Number.isFinite(Date.parse(tab.createdAt)) || typeof tab.updatedAt !== 'string' || !Number.isFinite(Date.parse(tab.updatedAt)))) throw new Error('browser_response_invalid');
    if (seen.has(row.sessionPath)) throw new Error('browser_response_invalid');
    seen.add(row.sessionPath);
    return row;
  });
}
export async function rustBrowserRequest(context: RustBrowserViewerContext, action: RustBrowserAction | 'workspaces' | 'stop', options?: { tabId?: string; url?: string }): Promise<unknown> {
  const platform = usePlatform();
  if (!platform.rustBrowserRequest) throw new Error('browser_desktop_unavailable');
  return platform.rustBrowserRequest(context, action, options);
}
