import { useStore } from '../stores';

export type ChatCardNavigationRequest = {
  kind: 'terminal' | 'subagent';
  ids: string[];
  /**
   * 发起导航时的会话。pending 重投只在它仍是当前会话时生效——折叠卡挂载可能晚于
   * 一次会话切换，没有作用域的 pending 会在错误的会话里弹出卡片。
   */
  sessionPath?: string | null;
};

type Listener = (request: ChatCardNavigationRequest) => boolean;

/** pending 导航只在这个时间窗内等待卡片挂载，超时丢弃，避免无限期残留。 */
export const PENDING_NAVIGATION_TTL_MS = 30_000;

const listeners = new Set<Listener>();
let pending: { request: ChatCardNavigationRequest; at: number } | null = null;

function deliver(request: ChatCardNavigationRequest): boolean {
  for (const listener of listeners) {
    if (listener(request)) return true;
  }
  return false;
}

function pendingUsable(entry: { request: ChatCardNavigationRequest; at: number }): boolean {
  if (Date.now() - entry.at > PENDING_NAVIGATION_TTL_MS) return false;
  const scope = entry.request.sessionPath;
  if (!scope) return true;
  return useStore.getState().currentSessionPath === scope;
}

function takePendingIfUsable(): ChatCardNavigationRequest | null {
  if (!pending) return null;
  if (!pendingUsable(pending)) {
    pending = null;
    return null;
  }
  return pending.request;
}

export function navigateToChatCard(request: ChatCardNavigationRequest): void {
  const ids = request.ids.filter((id) => typeof id === 'string' && id.trim());
  if (!ids.length) return;
  const normalized = { ...request, ids };
  pending = deliver(normalized) ? null : { request: normalized, at: Date.now() };
}

export function subscribeChatCardNavigation(listener: Listener): () => void {
  listeners.add(listener);
  const replay = takePendingIfUsable();
  if (replay && listener(replay)) pending = null;
  return () => listeners.delete(listener);
}
