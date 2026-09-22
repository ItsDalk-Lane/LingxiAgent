// @vitest-environment jsdom
/** 真实 chat WS 回调 → 本地 WebSocket 帧 → 真实接收分发/恢复 → buffer / Zustand。 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRequire } from 'node:module';
import { createChatRoute } from '../server/routes/chat.ts';
import { handleServerMessage, applyStreamingStatus } from '../desktop/src/react/services/ws-message-handler';
import { streamBufferManager } from '../desktop/src/react/hooks/use-stream-buffer';
import { useStore } from '../desktop/src/react/stores';
import { invalidateSessionStreamMeta, injectHandlers, injectWebSocketGetter, getSessionStreamMeta } from '../desktop/src/react/services/stream-resume';
import { resetSessionRefreshSchedulerForTest } from '../desktop/src/react/services/session-refresh-scheduler';
import { readLiveAssistantMessage } from '../desktop/src/react/stores/live-turn-store';
import type { WebSocket as NodeWebSocket } from 'ws';
const { WebSocket, WebSocketServer } = createRequire(import.meta.url)('ws') as typeof import('ws');
// 只替换 HTTP 外部读取边界：此容量/重连用例的持久历史为空；恢复、投影、store 都是真实现。
vi.mock('../desktop/src/react/hooks/use-hana-fetch', async importOriginal => ({
  ...await importOriginal<Record<string, unknown>>(),
  lingxiFetch: async () => new Response(JSON.stringify({ messages: [], hasMore: false, sessionFiles: [] })),
}));
const PATH = '/synthetic/f03-session.jsonl';
const ID = 'f03-session';
type Event = Record<string, unknown>;
type Wire = { readyState: number; send: (data: string) => void };
type Handlers = { onOpen: (event: object, ws: Wire) => void; onMessage: (event: { data: string }, ws: Wire) => void; onClose: (event: object, ws: Wire) => void };

function projection(): string {
  const items = useStore.getState().chatSessions[ID]?.items || [];
  return JSON.stringify(items.map(item => item.type === 'message'
    ? { ...item.data, live: readLiveAssistantMessage(PATH, item.data.id) } : item));
}

async function harness() {
  let factory: (ctx: object) => Handlers = () => { throw new Error('WS route not registered'); };
  let emit: (event: Event, path: string) => void = () => {};
  createChatRoute({
    agentName: 'Synthetic', getSessionByPath: () => ({ entries: [] }),
    getSessionIdForPath: () => ID,
    getSessionManifest: () => ({ currentLocator: { path: PATH } }),
    isSessionStreaming: () => false, isSessionSwitching: () => false,
    abortAllStreaming: async () => {}, steerSession: () => false, slashDispatcher: null,
  }, { subscribe: (fn: typeof emit) => { emit = fn; }, send: async () => {}, eventBus: { emit: () => {} } }, {
    upgradeWebSocket: (fn: typeof factory) => { factory = fn; return () => new Response(null); },
    disconnectAbortGraceMs: 0,
  });
  const server = new WebSocketServer({ host: '127.0.0.1', port: 0 });
  await new Promise<void>(resolve => server.once('listening', resolve));
  const address = server.address();
  if (typeof address === 'string' || !address) throw new Error('missing local port');
  let serverSocket: NodeWebSocket;
  server.on('connection', ws => {
    serverSocket = ws;
    const handlers = factory({});
    handlers.onOpen({}, ws);
    ws.on('message', data => handlers.onMessage({ data: data.toString() }, ws));
    ws.on('close', () => handlers.onClose({}, ws));
  });
  const received: Event[] = [];
  const client = new WebSocket(`ws://127.0.0.1:${address.port}`);
  client.on('message', data => {
    const event = JSON.parse(data.toString()) as Event;
    received.push(event);
    handleServerMessage(event);
  });
  await new Promise<void>(resolve => client.once('open', resolve));
  injectHandlers(handleServerMessage, applyStreamingStatus);
  injectWebSocketGetter(() => client as unknown as globalThis.WebSocket);
  const drain = async () => { await new Promise<void>(resolve => {
    client.ping(); serverSocket.once('ping', () => setTimeout(resolve, 10));
  }); };
  const model = (text: string) => {
    const message = { role: 'assistant', api: 'anthropic-messages', content: [{ type: 'text', text }] };
    emit({ type: 'turn_start' }, PATH);
    emit({ type: 'message_start', message }, PATH);
    emit({ type: 'message_update', message, assistantMessageEvent: { type: 'text_delta', contentIndex: 0, delta: text, partial: message } }, PATH);
    emit({ type: 'message_update', message, assistantMessageEvent: { type: 'text_end', contentIndex: 0, content: text, partial: message } }, PATH);
    emit({ type: 'message_end', message }, PATH);
    emit({ type: 'turn_end', message, toolResults: [] }, PATH);
  };
  return { received, model, drain, resume: () => client.send(JSON.stringify({ type: 'resume_stream', sessionPath: PATH, sessionId: ID, streamId: null, sinceSeq: 0 })), emit: (event: Event) => emit(event, PATH), inject: (event: Event) => serverSocket.send(JSON.stringify(event)),
    close: async () => { client.close(); await new Promise<void>(resolve => server.close(() => resolve())); } };
}

beforeEach(() => {
  streamBufferManager.clearAll(); invalidateSessionStreamMeta();
  useStore.setState({ currentSessionId: null, currentSessionPath: null,
    sessions: [{ path: PATH, sessionId: ID }], sessionLocatorsById: { [ID]: { path: PATH } },
    chatSessions: {}, streamingSessions: [], activeSessionStreams: {} } as never);
  useStore.getState().initSession(PATH, [{ type: 'message', data: { id: 'u1', role: 'user', text: 'synthetic' } }], false);
});
afterEach(() => { resetSessionRefreshSchedulerForTest(); streamBufferManager.clearAll(); invalidateSessionStreamMeta(); injectWebSocketGetter(() => null); });

describe('F03 real WS route and renderer consumption', () => {
  it('C14/C15/C17: rejects old start/delta/end/tool/status frames and preserves multiple model turns in the current run', async () => {
    const h = await harness();
    try {
      h.emit({ type: 'agent_start' }); h.model('A正文'); await h.drain();
      h.resume(); await h.drain();
      h.emit({ type: 'agent_settled' }); await h.drain();
      const oldFrames = h.received.filter(e => e.streamId);
      const oldStart = oldFrames.find(e => e.type === 'assistant_run_start')!;
      h.emit({ type: 'agent_start' }); h.model('B第一模型'); h.model('B第二模型'); await h.drain();
      const currentStart = h.received.filter(e => e.type === 'assistant_run_start').at(-1)!;
      expect(currentStart.streamId).not.toBe(oldStart.streamId);
      expect(currentStart.runId).not.toBe(currentStart.streamId);
      for (const event of oldFrames) h.inject(event);
      for (const type of ['assistant_segment_start', 'assistant_segment_delta', 'assistant_segment_end', 'tool_start', 'tool_end', 'status', 'assistant_run_start', 'assistant_run_end']) {
        h.inject({ ...oldStart, type, seq: 900, segmentId: 'stale', delta: '旧流污染', name: 'stale_tool', id: 'stale-tool', isStreaming: false });
      }
      await h.drain();
      expect(getSessionStreamMeta(PATH)?.streamId).toBe(currentStart.streamId);
      expect(streamBufferManager.isRunActive(PATH)).toBe(true);
      expect(useStore.getState().streamingSessions).toContain(ID);
      h.emit({ type: 'agent_settled' }); await h.drain();
      expect(projection()).toContain('B第一模型'); expect(projection()).toContain('B第二模型');
      expect(projection()).not.toContain('旧流污染'); expect(projection()).not.toContain('stale_tool');
      expect(streamBufferManager.isRunActive(PATH)).toBe(false);
    } finally { await h.close(); }
  });

  it('C16/C18: duplicate frames are idempotent, a gap resumes through the real route, and untagged old frames cannot acquire current identity', async () => {
    const h = await harness();
    try {
      h.emit({ type: 'agent_start' }); h.model('只出现一次'); await h.drain();
      const delta = h.received.find(e => e.type === 'assistant_segment_delta')!;
      h.inject(delta);
      h.inject({ ...delta, seq: Number(delta.seq) + 100, delta: '有缺口不能先写' });
      h.inject({ type: 'text_delta', sessionPath: PATH, delta: '无身份污染' });
      await h.drain(); await h.drain();
      expect(h.received.some(e => e.type === 'stream_resume')).toBe(true);
      expect(projection()).not.toContain('有缺口不能先写'); expect(projection()).not.toContain('无身份污染');
      h.emit({ type: 'agent_settled' }); await h.drain();
      const text = projection();
      expect(text).toContain('只出现一次'); expect(text).not.toContain('只出现一次只出现一次');
    } finally { await h.close(); }
  });
  it('C16: more than 256 runs recover through the real route without accepting previously retired starts', async () => {
    const h = await harness();
    try {
      for (let i = 0; i < 260; i++) {
        h.emit({ type: 'agent_start' }); h.model(`round-${i}`);
        if (i < 259) h.emit({ type: 'agent_settled' });
        await h.drain();
      }
      await vi.waitFor(() => expect(projection()).toContain('round-259'));
      expect(h.received.some(e => e.type === 'stream_resume')).toBe(true);
      const first = h.received.find(e => e.type === 'assistant_run_start')!;
      const last = h.received.filter(e => e.type === 'assistant_run_start').at(-1)!;
      h.inject(first); await h.drain();
      expect(getSessionStreamMeta(PATH)?.streamId).toBe(last.streamId);
      expect(projection()).toContain('round-259');
    } finally { await h.close(); }
  });

});

it('keeps a resolved confirmation resolved after the next real streamed projection', async () => {
  useStore.setState({ currentSessionId: ID, currentSessionPath: PATH });
  const h = await harness();
  try {
    h.emit({ type: 'agent_start' });
    h.emit({ type: 'session_confirmation', request: { type: 'session_confirmation', confirmId: 'f03-confirm', status: 'pending', surface: 'input' } });
    await h.drain();
    h.emit({ type: 'confirmation_resolved', confirmId: 'f03-confirm', action: 'confirmed' });
    await h.drain();
    h.model('审批完成后的继续输出');
    await h.drain();
    const confirmation = streamBufferManager.snapshot(PATH)?.blocks.find(block => block.type === 'session_confirmation');
    expect(confirmation).toMatchObject({ confirmId: 'f03-confirm', status: 'confirmed' });
    expect(projection()).not.toContain('"status":"pending"');
    h.emit({ type: 'agent_settled' }); await h.drain();
    expect(projection()).not.toContain('"status":"pending"');
    expect(projection()).toContain('"status":"confirmed"');
  } finally { await h.close(); }
});
