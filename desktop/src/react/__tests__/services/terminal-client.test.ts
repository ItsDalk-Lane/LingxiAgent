import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  configureTerminalClientWebSocketGetter,
  requestTerminalSnapshot,
  requestTerminalTail,
} from '../../services/terminal-client';

afterEach(() => configureTerminalClientWebSocketGetter(() => null));

describe('terminal client', () => {
  it('fails without an open websocket instead of queueing an unscoped request', () => {
    configureTerminalClientWebSocketGetter(() => null);
    expect(requestTerminalSnapshot({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
    })).toBe(false);
  });

  it('sends snapshot and tail requests with stable session identity', () => {
    const socket = { readyState: 1, send: vi.fn() };
    configureTerminalClientWebSocketGetter(() => socket as unknown as WebSocket);

    expect(requestTerminalSnapshot({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
    })).toBe(true);
    expect(requestTerminalTail({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminalId: 'term_1',
      sinceSeq: 4,
    })).toBe(true);
    expect(requestTerminalTail({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminalId: 'term_2',
      sinceSeq: null,
    })).toBe(true);

    expect(socket.send.mock.calls.map(([raw]) => JSON.parse(raw))).toEqual([
      {
        type: 'terminal_snapshot_request',
        sessionId: 'sess_a',
        sessionPath: '/sessions/a.jsonl',
      },
      {
        type: 'terminal_tail_request',
        sessionId: 'sess_a',
        sessionPath: '/sessions/a.jsonl',
        terminalId: 'term_1',
        sinceSeq: 4,
      },
      {
        type: 'terminal_tail_request',
        sessionId: 'sess_a',
        sessionPath: '/sessions/a.jsonl',
        terminalId: 'term_2',
      },
    ]);
  });
});
