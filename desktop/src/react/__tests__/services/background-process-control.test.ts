import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  configureBackgroundProcessWebSocketGetter,
  handleBackgroundProcessControlResult,
  stopSubagentProcess,
  stopTerminalProcess,
} from '../../services/background-process-control';

afterEach(() => configureBackgroundProcessWebSocketGetter(() => null));

describe('background process control', () => {
  it('rejects when no local websocket is open', async () => {
    await expect(stopTerminalProcess({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminalId: 'term_1',
    })).rejects.toThrow('process_control_disconnected');
  });

  it('pairs a terminal stop response by request id', async () => {
    const socket = { readyState: 1, send: vi.fn() };
    configureBackgroundProcessWebSocketGetter(() => socket as unknown as WebSocket);

    const resultPromise = stopTerminalProcess({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminalId: 'term_1',
    });
    const request = JSON.parse(socket.send.mock.calls[0][0]);

    handleBackgroundProcessControlResult({
      type: 'terminal_close_result',
      requestId: request.requestId,
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminalId: 'term_1',
      status: 'killed',
    });

    await expect(resultPromise).resolves.toMatchObject({ status: 'killed' });
    expect(request).toMatchObject({
      type: 'terminal_close_request',
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminalId: 'term_1',
    });
  });

  it('surfaces a rejected subagent stop instead of pretending it succeeded', async () => {
    const socket = { readyState: 1, send: vi.fn() };
    configureBackgroundProcessWebSocketGetter(() => socket as unknown as WebSocket);
    const resultPromise = stopSubagentProcess({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      taskId: 'task_other',
    });
    const request = JSON.parse(socket.send.mock.calls[0][0]);

    handleBackgroundProcessControlResult({
      type: 'subagent_stop_result',
      requestId: request.requestId,
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      taskId: 'task_other',
      status: 'rejected',
      reason: 'session_mismatch',
    });

    await expect(resultPromise).rejects.toThrow('session_mismatch');
  });
});
