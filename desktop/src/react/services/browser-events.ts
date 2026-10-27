import { useStore } from '../stores';
import { browserStateForPath, setBrowserStateForPath } from '../stores/browser-slice';

/** 服务端事件与即时消息共用同一浏览器状态入口。 */
export function applyBrowserStatus(event: Record<string, unknown>): void {
  const bsp = event.sessionPath;
  if (typeof bsp !== 'string' || !bsp) return;
  const bRunning = !!event.running;
  const bUrl = typeof event.url === 'string' ? event.url : null;
  const prev = browserStateForPath(useStore.getState(), bsp);
  const incomingThumbnail = typeof event.thumbnail === 'string' ? event.thumbnail : null;
  const hasFreshThumbnail = bRunning && typeof event.thumbnail === 'string' && event.thumbnail.length > 0;
  const bThumbnail = bRunning ? (hasFreshThumbnail ? incomingThumbnail : prev?.thumbnail ?? null) : null;
  const thumbnailCapturedAt = bRunning
    ? hasFreshThumbnail
      ? (typeof event.thumbnailCapturedAt === 'number' ? event.thumbnailCapturedAt : Date.now())
      : prev?.thumbnailCapturedAt ?? null
    : null;
  const thumbnailUrl = bRunning
    ? hasFreshThumbnail
      ? (typeof event.thumbnailUrl === 'string' ? event.thumbnailUrl : bUrl)
      : prev?.thumbnailUrl ?? null
    : null;
  const thumbnailFresh = bRunning && hasFreshThumbnail;
  // 卡片的"收起"是用户意图，状态更新不该把它抹掉；只有浏览器重新启用（running false→true）
  // 才算新一轮会话，卡片回归。
  const collapsed = bRunning && !prev?.running ? false : (prev?.collapsed ?? false);
  // 记录最近活跃时间：切到其他会话时，聊天区卡片接力显示「最近在用」的浏览器
  const lastActiveAt = bRunning ? Date.now() : (prev?.lastActiveAt ?? null);
  setBrowserStateForPath(bsp, { executionNode: typeof event.executionNode === 'string' ? event.executionNode : undefined, running: bRunning, url: bUrl, thumbnail: bThumbnail, thumbnailCapturedAt, thumbnailUrl, thumbnailFresh, collapsed, lastActiveAt });
}
