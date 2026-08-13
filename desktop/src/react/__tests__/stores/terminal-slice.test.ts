import { describe, expect, it } from 'vitest';
import { createTerminalSlice, selectTerminalById, selectTerminals, type TerminalSlice } from '../../stores/terminal-slice';
import type { SessionLocatorState } from '../../stores/session-slice';

function terminal(overrides: Record<string, unknown> = {}) {
  return {
    terminalId: 'term_1',
    sessionId: 'sess_a',
    sessionPath: '/sessions/a.jsonl',
    agentId: 'hana',
    cwd: '/workspace',
    command: 'npm test',
    label: 'tests',
    status: 'running' as const,
    seq: 0,
    createdAt: 1,
    lastActivityAt: 1,
    exitedAt: null,
    exitCode: null,
    signal: null,
    transcriptPath: '/state/term_1.jsonl',
    ...overrides,
  };
}

function makeStore(initial: SessionLocatorState = {}) {
  let state: TerminalSlice & SessionLocatorState = {
    currentSessionId: null,
    currentSessionPath: null,
    sessions: [],
    sessionLocatorsById: {},
    ...initial,
  } as TerminalSlice & SessionLocatorState;
  const set = (update: Partial<TerminalSlice> | ((current: TerminalSlice & SessionLocatorState) => Partial<TerminalSlice>)) => {
    const partial = typeof update === 'function' ? update(state) : update;
    state = { ...state, ...partial };
  };
  state = { ...state, ...createTerminalSlice(set) };
  return { getState: () => state };
}

describe('terminal slice', () => {
  it('replaces a session snapshot and removes entries absent from the new snapshot', () => {
    const store = makeStore({
      currentSessionId: 'sess_a',
      currentSessionPath: '/sessions/a.jsonl',
      sessionLocatorsById: { sess_a: { path: '/sessions/a.jsonl' } },
    });

    store.getState().replaceTerminalSnapshot({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminals: [terminal(), terminal({ terminalId: 'term_2' })],
    });
    expect(selectTerminals('/sessions/a.jsonl')(store.getState())).toHaveLength(2);

    store.getState().replaceTerminalSnapshot({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminals: [terminal({ terminalId: 'term_2', status: 'exited', exitCode: 0 })],
    });
    expect(selectTerminals('/sessions/a.jsonl')(store.getState())).toEqual([
      terminal({ terminalId: 'term_2', status: 'exited', exitCode: 0 }),
    ]);
  });

  it('upserts state by terminal id without crossing sessions', () => {
    const store = makeStore({
      sessions: [
        { sessionId: 'sess_a', path: '/sessions/a.jsonl' },
        { sessionId: 'sess_b', path: '/sessions/b.jsonl' },
      ],
      sessionLocatorsById: {
        sess_a: { path: '/sessions/a.jsonl' },
        sess_b: { path: '/sessions/b.jsonl' },
      },
    });
    store.getState().upsertTerminal(terminal());
    store.getState().upsertTerminal(terminal({
      terminalId: 'term_b',
      sessionId: 'sess_b',
      sessionPath: '/sessions/b.jsonl',
    }));
    store.getState().upsertTerminal(terminal({ status: 'exited', exitCode: 7, seq: 4 }));

    expect(selectTerminals('/sessions/a.jsonl')(store.getState())).toEqual([
      terminal({ status: 'exited', exitCode: 7, seq: 4 }),
    ]);
    expect(selectTerminals('/sessions/b.jsonl')(store.getState())).toEqual([
      terminal({ terminalId: 'term_b', sessionId: 'sess_b', sessionPath: '/sessions/b.jsonl' }),
    ]);
  });

  it('keeps terminal metadata reachable after a path relocation with the same session id', () => {
    const store = makeStore({
      currentSessionId: 'sess_a',
      currentSessionPath: '/sessions/old.jsonl',
      sessionLocatorsById: { sess_a: { path: '/sessions/old.jsonl' } },
    });
    store.getState().replaceTerminalSnapshot({
      sessionId: 'sess_a',
      sessionPath: '/sessions/old.jsonl',
      terminals: [terminal({ sessionPath: '/sessions/old.jsonl' })],
    });

    Object.assign(store.getState(), {
      currentSessionPath: '/sessions/archive/new.jsonl',
      sessionLocatorsById: { sess_a: { path: '/sessions/archive/new.jsonl' } },
    });

    expect(selectTerminals('/sessions/archive/new.jsonl')(store.getState())).toEqual([
      terminal({ sessionPath: '/sessions/old.jsonl' }),
    ]);
    store.getState().clearTerminals('/sessions/archive/new.jsonl');
    expect(selectTerminals('/sessions/archive/new.jsonl')(store.getState())).toEqual([]);
  });

  it('finds a terminal by id across sessions (subagent preview renders with the parent path)', () => {
    const store = makeStore({
      sessions: [
        { sessionId: 'sess_a', path: '/sessions/a.jsonl' },
        { sessionId: 'sess_sub', path: '/sessions/sub.jsonl' },
      ],
      sessionLocatorsById: {
        sess_a: { path: '/sessions/a.jsonl' },
        sess_sub: { path: '/sessions/sub.jsonl' },
      },
    });
    store.getState().upsertTerminal(terminal({
      terminalId: 'term_sub',
      sessionId: 'sess_sub',
      sessionPath: '/sessions/sub.jsonl',
    }));

    // 渲染上下文（父会话 path）下按 id 仍能命中注册在子会话桶里的终端。
    expect(selectTerminalById('term_sub')(store.getState())).toEqual(
      terminal({ terminalId: 'term_sub', sessionId: 'sess_sub', sessionPath: '/sessions/sub.jsonl' }),
    );
    expect(selectTerminalById('term_missing')(store.getState())).toBeNull();
    expect(selectTerminalById(null)(store.getState())).toBeNull();
  });
});
