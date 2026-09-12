import { useCallback, useEffect, useState } from 'react';
import { useStore } from '../../stores';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { useScopedSessionPath } from '../session-scope-context';

/**
 * composer-memory-mode.ts — 「本次会话是否使用长期记忆」的会话归属。
 *
 * 全局 store.memoryEnabled 是主聊天页 / 新建会话草稿的开关（既有行为不动）。
 * 侧边对话面板需要自己的开关：侧边会话创建时默认关闭记忆（旁支讨论不写入
 * 长期记忆），用户可以在侧栏内自行打开。读写都按会话走
 * /api/sessions/memory（服务端语义与建会话时的 memoryEnabled 同源）。
 */

interface MemoryQueryState {
  status: 'loading' | 'ready' | 'error';
  enabled: boolean;
  /** 读取时刻：超过 TTL 的缓存重新向服务端确认，避免展示过期状态。 */
  fetchedAt: number;
}

/** 缓存 TTL：刚读过的状态直接复用，过期后重新确认（用户可能在别处改过开关）。 */
const CACHE_TTL_MS = 30_000;

const queryCache = new Map<string, MemoryQueryState>();

function readMemoryCache(sessionPath: string): MemoryQueryState | undefined {
  const entry = queryCache.get(sessionPath);
  if (!entry) return undefined;
  if (entry.status === 'error') return entry;
  if (Date.now() - entry.fetchedAt > CACHE_TTL_MS) return undefined;
  return entry;
}

/**
 * 面板关闭 / 会话被替换时丢弃缓存：下一次打开一定重新向服务端确认，
 * 不让上一个会话或上一次运行的结论残留。
 */
export function resetSessionMemoryCache(sessionPath?: string | null): void {
  if (sessionPath) queryCache.delete(sessionPath);
  else queryCache.clear();
}

/** 侧边会话创建时已知的初始记忆状态：直接写入缓存，省一次读请求。 */
export function noteSessionMemoryEnabled(sessionPath: string, enabled: boolean): void {
  queryCache.set(sessionPath, { status: 'ready', enabled, fetchedAt: Date.now() });
}

export function useScopedMemoryEnabled(): {
  enabled: boolean;
  /** 是否应当渲染记忆开关：主聊天页沿用 Welcome 页既有入口，不在工具栏重复。 */
  showToggle: boolean;
  setEnabled: (enabled: boolean) => void;
} {
  const scopedSessionPath = useScopedSessionPath();
  const globalEnabled = useStore(s => s.memoryEnabled);
  const isSideScope = useStore(s => !!scopedSessionPath && s.currentSessionPath !== scopedSessionPath);
  const [cacheTick, setCacheTick] = useState(0);

  useEffect(() => {
    if (!isSideScope || !scopedSessionPath) return undefined;
    const cached = readMemoryCache(scopedSessionPath);
    if (cached && cached.status !== 'error') return undefined;
    void (async () => {
      try {
        const res = await lingxiFetch(`/api/sessions/memory?path=${encodeURIComponent(scopedSessionPath)}`, {
          throwOnHttpError: false,
        });
        const data = await res.json();
        if (res.ok && typeof data?.memoryEnabled === 'boolean') {
          queryCache.set(scopedSessionPath, { status: 'ready', enabled: data.memoryEnabled, fetchedAt: Date.now() });
        } else {
          // 读不到就按「关闭」保守呈现：未确认开启的记忆开关不会伪装成已开启。
          queryCache.set(scopedSessionPath, { status: 'error', enabled: false, fetchedAt: Date.now() });
        }
      } catch (err) {
        console.warn('[side-chat] session memory read failed:', err);
        queryCache.set(scopedSessionPath, { status: 'error', enabled: false, fetchedAt: Date.now() });
      } finally {
        // 读取结果写入模块缓存后强制重算；组件已卸载时这次 setState 由 React 忽略。
        setCacheTick(tick => tick + 1);
      }
    })();
  }, [cacheTick, isSideScope, scopedSessionPath]);

  const setEnabled = useCallback((enabled: boolean) => {
    if (!isSideScope || !scopedSessionPath) {
      useStore.setState({ memoryEnabled: enabled });
      return;
    }
    const previous = readMemoryCache(scopedSessionPath) ?? { status: 'loading' as const, enabled: false, fetchedAt: 0 };
    queryCache.set(scopedSessionPath, { status: 'ready', enabled, fetchedAt: Date.now() });
    setCacheTick(tick => tick + 1);
    void (async () => {
      try {
        const res = await lingxiFetch('/api/sessions/memory', {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ path: scopedSessionPath, memoryEnabled: enabled }),
          throwOnHttpError: false,
        });
        const data = await res.json();
        if (res.ok && typeof data?.memoryEnabled === 'boolean') {
          queryCache.set(scopedSessionPath, { status: 'ready', enabled: data.memoryEnabled, fetchedAt: Date.now() });
        } else {
          throw new Error(data?.error || res.statusText || 'session memory update failed');
        }
      } catch (err) {
        console.warn('[side-chat] session memory update failed:', err);
        queryCache.set(scopedSessionPath, previous);
      } finally {
        setCacheTick(tick => tick + 1);
      }
    })();
  }, [isSideScope, scopedSessionPath]);

  const cached = isSideScope && scopedSessionPath ? readMemoryCache(scopedSessionPath) : undefined;
  // cacheTick 参与结果计算：缓存变化后强制重算（Map 不是 React 状态）。
  void cacheTick;
  return {
    enabled: isSideScope ? (cached?.enabled ?? false) : globalEnabled,
    showToggle: isSideScope,
    setEnabled,
  };
}
