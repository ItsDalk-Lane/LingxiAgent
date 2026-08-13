import { describe, expect, it, vi } from 'vitest';

import {
  requestTerminalSnapshotForCurrentSession,
  resolveStreamingSessionResumeTargets,
} from '../../services/websocket';

const terminalClientMocks = vi.hoisted(() => ({
  requestTerminalSnapshot: vi.fn(),
}));

vi.mock('../../services/terminal-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../services/terminal-client')>();
  return { ...actual, requestTerminalSnapshot: terminalClientMocks.requestTerminalSnapshot };
});

describe('websocket session resume targets', () => {
  it('resolves sessionId-keyed streaming state back to current locators', () => {
    expect(resolveStreamingSessionResumeTargets({
      streamingSessions: ['sess_a', '/legacy.jsonl', 'sess_missing'],
      sessionLocatorsById: {
        sess_a: { path: '/sessions/a.jsonl' },
        sess_missing: { path: null },
      },
    } as never)).toEqual(['/sessions/a.jsonl', '/legacy.jsonl']);
  });

  it('requests a terminal snapshot for the current stable session after reconnect', () => {
    terminalClientMocks.requestTerminalSnapshot.mockReturnValue(true);

    expect(requestTerminalSnapshotForCurrentSession({
      currentSessionId: 'sess_a',
      currentSessionPath: '/sessions/a.jsonl',
    })).toBe(true);
    expect(terminalClientMocks.requestTerminalSnapshot).toHaveBeenCalledWith({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
    });
  });
});
