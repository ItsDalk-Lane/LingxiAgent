import { beforeEach, describe, expect, it, vi } from 'vitest';

const terminalStreamMocks = vi.hoisted(() => ({
  handleChunks: vi.fn(),
  handleTail: vi.fn(),
}));

vi.mock('../../services/terminal-output-stream', () => ({
  terminalOutputStream: terminalStreamMocks,
}));

import { handleServerMessage } from '../../services/ws-message-handler';
import { useStore } from '../../stores';
import { selectTerminals } from '../../stores/terminal-slice';

function terminal(overrides: Record<string, unknown> = {}) {
  return {
    terminalId: 'term_1',
    sessionId: 'sess_a',
    sessionPath: '/sessions/a.jsonl',
    agentId: 'hana',
    cwd: '/workspace',
    command: 'npm test',
    label: 'tests',
    status: 'running',
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

describe('terminal websocket message handler', () => {
  beforeEach(() => {
    terminalStreamMocks.handleChunks.mockClear();
    terminalStreamMocks.handleTail.mockClear();
    useStore.setState({
      currentSessionId: 'sess_a',
      currentSessionPath: '/sessions/a.jsonl',
      sessions: [{ sessionId: 'sess_a', path: '/sessions/a.jsonl' }],
      sessionLocatorsById: { sess_a: { path: '/sessions/a.jsonl' } },
      terminalsBySession: {},
    } as never);
  });

  it('routes snapshots and lifecycle state into the low-frequency terminal slice', () => {
    handleServerMessage({
      type: 'terminal_snapshot',
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminals: [terminal(), terminal({ terminalId: 'term_2' })],
    });
    handleServerMessage({
      type: 'terminal_state',
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminal: terminal({ status: 'exited', exitCode: 0, seq: 5 }),
    });

    expect(selectTerminals('/sessions/a.jsonl')(useStore.getState())).toEqual([
      terminal({ status: 'exited', exitCode: 0, seq: 5 }),
      terminal({ terminalId: 'term_2' }),
    ]);
  });

  it('routes high-frequency output only to the lightweight stream without Zustand writes', () => {
    const message = {
      type: 'terminal_output',
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminalId: 'term_1',
      chunks: [{ seq: 1, data: 'live' }],
    };
    const setState = vi.spyOn(useStore, 'setState');

    handleServerMessage(message);

    expect(terminalStreamMocks.handleChunks).toHaveBeenCalledWith(message);
    expect(setState).not.toHaveBeenCalled();
    setState.mockRestore();
  });

  it('routes terminal tail responses to the lightweight stream', () => {
    const message = {
      type: 'terminal_tail',
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminalId: 'term_1',
      terminal: terminal({ seq: 1 }),
      chunks: [{ seq: 1, data: 'tail' }],
      sinceSeq: null,
      lastSeq: 1,
      truncated: false,
    };

    handleServerMessage(message);

    expect(terminalStreamMocks.handleTail).toHaveBeenCalledWith(message);
  });

  it('throttles identity-mismatch warnings from high-frequency terminal output', () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(1_000_000);
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
      // locator 里 sess_a 的权威 path 与消息携带的 path 不符 → 每条都被丢弃，
      // 但告警必须节流（每键每分钟最多一条），不能每块输出刷一条。
      useStore.setState({
        sessionLocatorsById: { sess_a: { path: '/sessions/real.jsonl' } },
      } as never);
      const mismatched = (seq: number) => ({
        type: 'terminal_output',
        sessionId: 'sess_a',
        sessionPath: '/sessions/other.jsonl',
        terminalId: 'term_1',
        chunks: [{ seq, data: 'x' }],
      });

      for (let seq = 1; seq <= 5; seq += 1) handleServerMessage(mismatched(seq));
      expect(warn).toHaveBeenCalledTimes(1);
      expect(terminalStreamMocks.handleChunks).not.toHaveBeenCalled();

      vi.setSystemTime(1_000_000 + 61_000);
      handleServerMessage(mismatched(6));
      expect(warn).toHaveBeenCalledTimes(2);
      warn.mockRestore();
    } finally {
      vi.useRealTimers();
    }
  });
});
