import { useCallback, useEffect, useState } from 'react';
import { useStore } from '../../stores';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { useScopedSessionPath } from '../session-scope-context';
import type { PermissionMode } from './PlanModeButton';

/**
 * composer-permission-mode.ts — 权限模式（自动/操作/询问/只读）的会话归属。
 *
 * 既有实现把模式存在全局 store.sessionPermissionMode（= 主会话）。侧边对话
 * 面板与主会话同时存在时，两边必须是两份模式：
 * - 主聊天页（无作用域）：读全局，写回全局（行为与改动前完全一致）。
 * - 侧边面板：模式属于侧边会话，只经 /api/session-permission-mode 的 sessionPath
 *   分支读写，绝不碰主会话的模式。
 */

const PERMISSION_MODES = new Set(['auto', 'operate', 'ask', 'read_only']);

const NO_MODE: PermissionMode | null = null;

function normalizeMode(value: unknown): PermissionMode | null {
  return typeof value === 'string' && PERMISSION_MODES.has(value) ? value as PermissionMode : null;
}

/** 侧边会话的权限模式：初值取会话投影（服务端按会话返回），切换后本地保存。 */
export function useScopedPermissionMode(): {
  mode: PermissionMode;
  setMode: (mode: PermissionMode) => void;
} {
  const scopedSessionPath = useScopedSessionPath();
  const globalMode = useStore(s => s.sessionPermissionMode);
  const setGlobalMode = useStore(s => s.setSessionPermissionMode);
  const projectionMode = useStore(s => {
    if (!scopedSessionPath || s.currentSessionPath === scopedSessionPath) return NO_MODE;
    const projection = s.sessions.find(session => session.path === scopedSessionPath);
    return normalizeMode((projection as { permissionMode?: unknown } | undefined)?.permissionMode);
  });
  const [sideMode, setSideMode] = useState<PermissionMode | null>(projectionMode);

  useEffect(() => {
    if (!scopedSessionPath || scopedSessionPath === useStore.getState().currentSessionPath) {
      setSideMode(null);
      return;
    }
    // 会话投影刷新（例如会话列表重载带回了服务端真实模式）时同步初值。
    if (projectionMode) setSideMode(projectionMode);
  }, [projectionMode, scopedSessionPath]);

  const isSideScope = useStore(s => !!scopedSessionPath && s.currentSessionPath !== scopedSessionPath);

  const setMode = useCallback((mode: PermissionMode) => {
    if (!isSideScope) {
      setGlobalMode(mode);
      return;
    }
    // 侧边面板：先落本地（按钮立即反映选择），服务端设置失败时回滚到确认值。
    setSideMode(mode);
    void (async () => {
      try {
        const res = await lingxiFetch('/api/session-permission-mode', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ mode, sessionPath: scopedSessionPath }),
        });
        const data = await res.json();
        const confirmed = normalizeMode(data?.mode);
        if (confirmed) setSideMode(confirmed);
      } catch (err) {
        console.warn('[side-chat] permission mode update failed:', err);
        const state = useStore.getState();
        const projection = state.sessions.find(session => session.path === scopedSessionPath);
        setSideMode(normalizeMode((projection as { permissionMode?: unknown } | undefined)?.permissionMode));
      }
    })();
  }, [isSideScope, scopedSessionPath, setGlobalMode]);

  return { mode: isSideScope ? (sideMode ?? projectionMode ?? 'ask') : globalMode, setMode };
}
