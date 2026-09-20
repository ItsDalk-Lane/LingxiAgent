import { sessionScopedKey, sessionScopedListIncludes, type SessionLocatorState } from './session-slice';
import type { ContextUsageBreakdown } from '../../../../shared/context-usage-breakdown.ts';

/** 按 session identity key 存储的 context usage 条目;breakdown 由服务端对账后下发,可为 null。 */
export interface ContextUsageEntry {
  tokens: number | null;
  window: number | null;
  percent: number | null;
  breakdown?: ContextUsageBreakdown | null;
}

export interface ContextSlice {
  /** Context usage — token count for the current session */
  contextTokens: number | null;
  contextWindow: number | null;
  contextPercent: number | null;
  /** 按 session identity key 存储的 context usage（读旧 path key 兼容） */
  contextBySession: Record<string, ContextUsageEntry>;
  /** Session identity keys currently undergoing compaction */
  compactingSessions: string[];
  /** Compaction mode for each busy session, keyed by session identity. */
  compactionModeBySession: Record<string, string>;
  /** 50% 压缩询问弹窗状态，按 session path 存储（服务端 compaction_suggested 事件写入）。 */
  compactionAskBySession: Record<string, { percent: number; askPercent: number; forcePercent: number }>;
  /** 用户确认「回复结束后自动压缩」的会话集合（session path）。 */
  pendingAutoCompactSessions: string[];
  addCompactionAsk: (path: string, info: { percent: number; askPercent: number; forcePercent: number }) => void;
  clearCompactionAsk: (path: string) => void;
  addPendingAutoCompact: (path: string) => void;
  removePendingAutoCompact: (path: string) => void;
  addCompactingSession: (path: string, mode?: string | null) => void;
  removeCompactingSession: (path: string) => void;
}

export const createContextSlice = (
  set: (partial: Partial<ContextSlice> | ((s: ContextSlice) => Partial<ContextSlice>)) => void
): ContextSlice => ({
  contextTokens: null,
  contextWindow: null,
  contextPercent: null,
  contextBySession: {},
  compactingSessions: [],
  compactionModeBySession: {},
  compactionAskBySession: {},
  pendingAutoCompactSessions: [],
  addCompactionAsk: (path, info) => set((s) => ({
    compactionAskBySession: {
      ...s.compactionAskBySession,
      [path]: {
        percent: info.percent,
        askPercent: info.askPercent,
        forcePercent: info.forcePercent,
      },
    },
  })),
  clearCompactionAsk: (path) => set((s) => {
    if (!s.compactionAskBySession[path]) return {};
    const next = { ...s.compactionAskBySession };
    delete next[path];
    return { compactionAskBySession: next };
  }),
  addPendingAutoCompact: (path) => set((s) => (
    s.pendingAutoCompactSessions.includes(path)
      ? {}
      : { pendingAutoCompactSessions: [...s.pendingAutoCompactSessions, path] }
  )),
  removePendingAutoCompact: (path) => set((s) => ({
    pendingAutoCompactSessions: s.pendingAutoCompactSessions.filter((item) => item !== path),
  })),
  addCompactingSession: (path, mode) => set((s) => {
    const key = sessionScopedKey(s as ContextSlice & SessionLocatorState, path) || path;
    const compactingSessions = s.compactingSessions.filter((item) => item !== key && item !== path);
    const compactionModeBySession = { ...s.compactionModeBySession };
    const normalizedMode = typeof mode === 'string' && mode.trim() ? mode.trim() : null;
    const existingMode = compactionModeBySession[key] || compactionModeBySession[path];
    delete compactionModeBySession[key];
    delete compactionModeBySession[path];
    if (normalizedMode || existingMode) {
      compactionModeBySession[key] = normalizedMode || existingMode;
    }
    return { compactingSessions: [...compactingSessions, key], compactionModeBySession };
  }),
  removeCompactingSession: (path) => set((s) => {
    const key = sessionScopedKey(s as ContextSlice & SessionLocatorState, path) || path;
    const compactionModeBySession = { ...s.compactionModeBySession };
    delete compactionModeBySession[key];
    delete compactionModeBySession[path];
    return {
      compactingSessions: s.compactingSessions.filter(p => p !== key && p !== path),
      compactionModeBySession,
    };
  }),
});

// ── Selectors ──
export const selectContextTokens = (s: ContextSlice) => s.contextTokens;
export const selectContextWindow = (s: ContextSlice) => s.contextWindow;
export const selectContextPercent = (s: ContextSlice) => s.contextPercent;

export function isSessionCompacting(
  state: ContextSlice & SessionLocatorState,
  sessionPath: string | null | undefined,
): boolean {
  return sessionScopedListIncludes(state, state.compactingSessions, sessionPath);
}

export function getSessionCompactionMode(
  state: ContextSlice & SessionLocatorState,
  sessionPath: string | null | undefined,
): string | null {
  if (!sessionPath || !isSessionCompacting(state, sessionPath)) return null;
  const key = sessionScopedKey(state, sessionPath);
  if (key && Object.prototype.hasOwnProperty.call(state.compactionModeBySession, key)) {
    return state.compactionModeBySession[key] || null;
  }
  return state.compactionModeBySession[sessionPath] || null;
}
