import type { TerminalPublicEntry } from '../../../../shared/terminal-ui-contract.ts';
import { sessionScopedKey, sessionScopedValue, type SessionLocatorState } from './session-slice';

export interface TerminalSnapshotPayload {
  sessionId?: string | null;
  sessionPath: string;
  terminals: TerminalPublicEntry[];
}

export interface TerminalSlice {
  /** 按稳定会话身份保存低频 terminal metadata；高频输出严禁进入这里。 */
  terminalsBySession: Record<string, TerminalPublicEntry[]>;
  replaceTerminalSnapshot: (snapshot: TerminalSnapshotPayload) => void;
  upsertTerminal: (terminal: TerminalPublicEntry) => void;
  clearTerminals: (sessionPath: string) => void;
}

type TerminalStoreState = TerminalSlice & SessionLocatorState;

function normalizedSessionId(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null;
}

function normalizeTerminalIdentity(
  terminal: TerminalPublicEntry,
  sessionId: string | null,
  sessionPath: string,
): TerminalPublicEntry {
  return { ...terminal, sessionId, sessionPath };
}

export const createTerminalSlice = (
  set: (partial: Partial<TerminalSlice> | ((state: TerminalStoreState) => Partial<TerminalSlice>)) => void,
): TerminalSlice => ({
  terminalsBySession: {},

  replaceTerminalSnapshot: ({ sessionId: rawSessionId, sessionPath, terminals }) => {
    if (!sessionPath) return;
    set((state) => {
      const sessionId = normalizedSessionId(rawSessionId)
        || normalizedSessionId((terminals || [])[0]?.sessionId)
        || null;
      const key = sessionId || sessionScopedKey(state, sessionPath) || sessionPath;
      const next = {
        ...state.terminalsBySession,
        [key]: (Array.isArray(terminals) ? terminals : [])
          .filter((terminal) => terminal?.terminalId)
          .map((terminal) => normalizeTerminalIdentity(terminal, sessionId, sessionPath)),
      };
      if (key !== sessionPath) delete next[sessionPath];
      return { terminalsBySession: next };
    });
  },

  upsertTerminal: (terminal) => {
    if (!terminal?.terminalId || !terminal?.sessionPath) return;
    set((state) => {
      const sessionId = normalizedSessionId(terminal.sessionId);
      const key = sessionId || sessionScopedKey(state, terminal.sessionPath) || terminal.sessionPath;
      const current = sessionScopedValue(state, state.terminalsBySession, terminal.sessionPath) || [];
      const normalized = normalizeTerminalIdentity(terminal, sessionId, terminal.sessionPath);
      const found = current.some((item) => item.terminalId === terminal.terminalId);
      const list = found
        ? current.map((item) => item.terminalId === terminal.terminalId ? { ...item, ...normalized } : item)
        : [...current, normalized];
      const next = { ...state.terminalsBySession, [key]: list };
      if (key !== terminal.sessionPath) delete next[terminal.sessionPath];
      return { terminalsBySession: next };
    });
  },

  clearTerminals: (sessionPath) => {
    if (!sessionPath) return;
    set((state) => {
      const key = sessionScopedKey(state, sessionPath) || sessionPath;
      if (!state.terminalsBySession[key] && !state.terminalsBySession[sessionPath]) return {};
      const next = { ...state.terminalsBySession };
      delete next[key];
      delete next[sessionPath];
      return { terminalsBySession: next };
    });
  },
});

const EMPTY_TERMINALS: TerminalPublicEntry[] = [];

export const selectTerminals =
  (sessionPath: string | null) =>
  (state: TerminalStoreState): TerminalPublicEntry[] => (
    sessionPath
      ? (sessionScopedValue(state, state.terminalsBySession, sessionPath) || EMPTY_TERMINALS)
      : EMPTY_TERMINALS
  );

/**
 * 按 terminalId 跨会话查找。终端注册在「真正起 tty 的会话」key 下，而渲染上下文
 * （如子助手预览）手里的 sessionPath 可能是父会话；terminalId 全局唯一，全量扫描
 * 低频 metadata 桶代价可忽略。
 */
export const selectTerminalById =
  (terminalId: string | null) =>
  (state: TerminalStoreState): TerminalPublicEntry | null => {
    if (!terminalId) return null;
    for (const list of Object.values(state.terminalsBySession || {})) {
      const hit = list.find((item) => item?.terminalId === terminalId);
      if (hit) return hit;
    }
    return null;
  };

export type { TerminalPublicEntry };
