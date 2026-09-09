import { expect, it, vi } from 'vitest';
import { createDesktopInputHistoryFixture } from './helpers/desktop-input-history-fixture.ts';
import { createChatRoute } from '../server/routes/chat.ts';

it('真实 SDK 输入回执与 run 结束事件保持同一落盘输入身份', async () => {
  const f = await createDesktopInputHistoryFixture();
  let createHandlers: any;
  let subscriber: any;
  const hub = {
    subscribe: vi.fn(fn => { subscriber = fn; }),
    send: vi.fn(async () => {}),
    eventBus: { emit: vi.fn() },
  };
  Object.assign(f.engine, {
    agentName: 'Synthetic', listSessions: vi.fn(async () => []), abortAllStreaming: vi.fn(async () => {}),
    isSessionSwitching: vi.fn(() => false), steerSession: vi.fn(() => false), slashDispatcher: null,
  });
  createChatRoute(f.engine, hub, { upgradeWebSocket: (factory: any) => {
    createHandlers = factory; return () => new Response(null);
  } } as any);
  const handlers = createHandlers({});
  const ws = { readyState: 1, send: vi.fn() };
  handlers.onOpen({}, ws);
  f.engine.emitEvent.mockImplementation((event: any, path: string) => subscriber(event, path));
  const unsubscribe = f.session.subscribe((event: any) => subscriber(event, f.sessionPath));
  try {
    expect(f.sessionId).not.toBe(f.manager.getSessionId());
    for (const text of ['wire-first', 'wire-second']) {
      ws.send.mockClear();
      await f.submit(text);
      const events = ws.send.mock.calls.map(([raw]: [string]) => JSON.parse(raw));
      const start = events.find(event => event.type === 'assistant_run_start');
      const ack = events.find(event => event.type === 'session_user_message' && event.message.sourceEntryId);
      const end = events.find(event => event.type === 'assistant_run_end');
      expect(start).toBeTruthy();
      expect(ack).toBeTruthy();
      expect(end).toBeTruthy();
      expect(ack.sessionId).toBe(f.sessionId);
      expect(end).toMatchObject({ sessionId: f.sessionId, runId: start.runId, streamId: start.streamId, turnInputEntryId: ack.message.sourceEntryId });
      expect(end.seq).toBeGreaterThan(start.seq);
    }
  } finally { unsubscribe(); }
});
