import { useStore } from '../stores';
import { createResourceEventClient, pauseRustResourceWatches, resubscribeActiveWatches } from './resource-events';
import {
  appendConnectionAuth,
  buildConnectionUrl,
  resolveServerConnection,
  type ServerConnection,
} from './server-connection';

/** 已接通的工作台入口使用同一连接的 Rust 服务，保留查询参数。 */
export function rustWorkbenchRoute(connection: { serverNodeKind?: string }, path: string): string {
  if (connection.serverNodeKind !== 'lingxi-service') return path;
  if (/^\/api\/preferences\/goal(?:\?|$)/.test(path)) return `/lingxi/v1${path.slice('/api'.length)}`;
  if (/^\/api\/mobile\/workbench\/(?:files|search|content|actions|upload)(?:\?|$)/.test(path)) {
    return `/lingxi/v1/workbench/${path.slice('/api/mobile/workbench/'.length)}`;
  }
  if (/^\/api\/(?:git\/(?:status|worktree-info|worktrees|branches|log|log-stats|file-diff|checkout|create-branch|stash|stashes|unstash|discard|worktree-create|commit|push|pull|stage|unstage|amend|fetch|ai-commit-message)|workbench\/(?:files|search|content|actions|upload)|studio\/workspaces(?:\/[^/?]+(?:\/files)?)?|desk\/(?:path|files|search-files|jian|logs)|diary\/(?:write|list)|resource-io\/(?:stat|read|list|search|write|write-expected-version|rename|move|trash|subscribe|subscriptions\/[^/?]+|watch|watch\/[^/?]+|watch-diagnostics|events))(?:\?|$)/.test(path)) {
    return `/lingxi/v1${path.slice('/api'.length)}`;
  }
  return path;
}

/** Rust 资源事件从正式游标接口补拉；切换连接或退出立即中止请求和定时器。 */
export function startRustResourceEvents(source: ServerConnection, options: {
  applyEvent: (event: Record<string, unknown>) => void;
  currentConnection?: () => ServerConnection | null;
  subscribeConnection?: (listener: () => void) => () => void;
  fetch?: typeof globalThis.fetch;
  reportError?: (error: unknown) => void;
}): () => void {
  if (source.serverNodeKind !== 'lingxi-service') return () => {};
  const key = JSON.stringify(source);
  const connection: ServerConnection = JSON.parse(key);
  const current = options.currentConnection ?? (() => resolveServerConnection(useStore.getState()));
  const fetchImpl = options.fetch ?? globalThis.fetch;
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending: AbortController | null = null;
  let requestTimeout: ReturnType<typeof setTimeout> | undefined;
  let unsubscribe: (() => void) | undefined;
  const stop = () => {
    if (stopped) return;
    stopped = true;
    if (timer) clearTimeout(timer);
    if (requestTimeout) clearTimeout(requestTimeout);
    pending?.abort();
    pauseRustResourceWatches(connection);
    unsubscribe?.();
    unsubscribe = undefined;
  };
  const owns = () => {
    if (!stopped && JSON.stringify(current()) === key) return true;
    stop();
    return false;
  };
  const client = createResourceEventClient({
    fetchImpl: async path => {
      if (!owns()) throw new Error('资源连接已切换');
      const controller = new AbortController();
      pending = controller;
      const timeout = setTimeout(() => controller.abort(), 30_000);
      requestTimeout = timeout;
      try {
        const response = await fetchImpl(buildConnectionUrl(connection, rustWorkbenchRoute(connection, path)), {
          headers: appendConnectionAuth(connection), signal: controller.signal,
        });
        if (!response.ok) throw new Error(`资源事件请求失败 (${response.status})`);
        const data = await response.json();
        if (!owns()) throw new Error('资源连接已切换');
        if (!data || typeof data.stale !== 'boolean' || !Number.isSafeInteger(data.latestSequence)
          || data.latestSequence < 0 || !Array.isArray(data.events)
          || (data.stale && data.events.length > 0)
          || data.events.some((event: Record<string, unknown>) => !event || typeof event.type !== 'string'
            || !Number.isSafeInteger(event.sequence) || Number(event.sequence) < 1 || Number(event.sequence) > data.latestSequence)) {
          throw new Error('资源事件返回格式无效');
        }
        return { json: async () => data };
      } finally {
        clearTimeout(timeout);
        if (requestTimeout === timeout) requestTimeout = undefined;
        if (pending === controller) pending = null;
      }
    },
    applyEvent: event => { if (owns()) options.applyEvent(event); },
    resubscribeWatches: () => owns() ? resubscribeActiveWatches() : undefined,
  });
  const poll = async () => {
    if (!owns()) return;
    try { await client.catchUpAfterReconnect(); }
    catch (error) { if (owns()) (options.reportError ?? console.warn)(error); }
    if (owns()) timer = setTimeout(poll, 1_000);
  };
  unsubscribe = (options.subscribeConnection ?? (listener => useStore.subscribe(listener)))(owns);
  // 服务重启后旧订阅可能已消失；仍打开的文件需要重建真实订阅。
  void resubscribeActiveWatches().catch(error => {
    if (owns()) (options.reportError ?? console.warn)(error);
  });
  void poll();
  return stop;
}
