// @vitest-environment jsdom
/** 真实 chat WS 回调 → 本地 WebSocket 帧 → 真实接收分发/恢复 → buffer / Zustand。 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRequire } from 'node:module';
import { createChatRoute } from '../server/routes/chat.ts';
import { handleServerMessage, applyStreamingStatus } from '../desktop/src/react/services/ws-message-handler';
import { streamBufferManager } from '../desktop/src/react/hooks/use-stream-buffer';
import { useStore } from '../desktop/src/react/stores';
import { invalidateSessionStreamMeta, injectHandlers, injectWebSocketGetter, getSessionStreamMeta, requestStreamResume, isStreamResumeRebuilding } from '../desktop/src/react/services/stream-resume';
import { resetSessionRefreshSchedulerForTest } from '../desktop/src/react/services/session-refresh-scheduler';
import { readLiveAssistantMessage } from '../desktop/src/react/stores/live-turn-store';
import type { WebSocket as NodeWebSocket } from 'ws';
const { WebSocket, WebSocketServer } = createRequire(import.meta.url)('ws') as typeof import('ws');

// 可控延迟的历史读取：只拦截 /api/sessions/messages，其余外部边界保持原 mock 语义。
const historyGate = vi.hoisted(() => {
  let armed = false;
  let waiters: Array<() => void> = [];
  return {
    arm: () => { armed = true; },
    release: () => {
      armed = false;
      const pending = waiters;
      waiters = [];
      for (const resolve of pending) resolve();
    },
    wait: () => (armed ? new Promise<void>(resolve => { waiters.push(resolve); }) : Promise.resolve()),
    pending: () => waiters.length,
  };
});
// 只替换 HTTP 外部读取边界：此容量/重连用例的持久历史为空；恢复、投影、store 都是真实现。
vi.mock('../desktop/src/react/hooks/use-hana-fetch', async importOriginal => ({
  ...await importOriginal<Record<string, unknown>>(),
  lingxiFetch: async (url: unknown) => {
    const target = typeof url === 'string' ? url : String((url as { url?: string })?.url ?? '');
    if (target.includes('/api/sessions/messages')) await historyGate.wait();
    return new Response(JSON.stringify({ messages: [], hasMore: false, sessionFiles: [] }));
  },
}));
const PATH = '/synthetic/f03-session.jsonl';
const ID = 'f03-session';
const OTHER_PATH = '/synthetic/f03-other.jsonl';
const OTHER_ID = 'f03-other';
type Event = Record<string, unknown>;
type Wire = { readyState: number; send: (data: string) => void };
type Handlers = { onOpen: (event: object, ws: Wire) => void; onMessage: (event: { data: string }, ws: Wire) => void; onClose: (event: object, ws: Wire) => void };

function projection(): string {
  const items = useStore.getState().chatSessions[ID]?.items || [];
  return JSON.stringify(items.map(item => item.type === 'message'
    ? { ...item.data, live: readLiveAssistantMessage(PATH, item.data.id) } : item));
}

async function harness(opts: { drop?: (event: Event) => boolean } = {}) {
  let drop: ((event: Event) => boolean) | null = opts.drop ?? null;
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
    if (drop && drop(event)) return;
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
    setDrop: (fn: ((event: Event) => boolean) | null) => { drop = fn; },
    close: async () => { client.close(); await new Promise<void>(resolve => server.close(() => resolve())); } };
}

beforeEach(() => {
  streamBufferManager.clearAll(); invalidateSessionStreamMeta();
  useStore.setState({ currentSessionId: null, currentSessionPath: null,
    sessions: [{ path: PATH, sessionId: ID }], sessionLocatorsById: { [ID]: { path: PATH } },
    chatSessions: {}, streamingSessions: [], activeSessionStreams: {} } as never);
  useStore.getState().initSession(PATH, [{ type: 'message', data: { id: 'u1', role: 'user', text: 'synthetic' } }], false);
});
afterEach(() => { resetSessionRefreshSchedulerForTest(); streamBufferManager.clearAll(); invalidateSessionStreamMeta(); injectWebSocketGetter(() => null); historyGate.release(); });

/** 把当前焦点切到另一个会话，让 PATH 成为后台会话（R2 组合用）。 */
function focusOtherSession(): void {
  useStore.setState({
    currentSessionId: OTHER_ID, currentSessionPath: OTHER_PATH,
    sessions: [{ path: PATH, sessionId: ID }, { path: OTHER_PATH, sessionId: OTHER_ID }],
    sessionLocatorsById: { [ID]: { path: PATH }, [OTHER_ID]: { path: OTHER_PATH } },
  } as never);
}

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

// ── R1：接纳不得先于序号校验产生退休副作用（recheck F03）──

describe('R1 admission atomicity over the real route', () => {
  it('a gap-rejected assistant_run_end retires nothing; the re-delivered seq3/seq4 complete the body, finalize exactly once, and only evidenced seqs enter the watermark', async () => {
    const h = await harness();
    try {
      h.emit({ type: 'agent_start' }); await h.drain();
      const runStart = h.received.find(e => e.type === 'assistant_run_start')!;
      const base = Number(runStart.seq);
      const S = runStart.streamId as string;
      const R = runStart.runId as string;
      // seq(base+1) segment_start 已达；(base+2) delta 传输丢失；(base+3) run_end 先到。
      h.inject({ ...runStart, type: 'assistant_segment_start', seq: base + 1, segmentId: 'seg-r1', kind: 'text' });
      h.inject({ ...runStart, type: 'assistant_run_end', seq: base + 3, status: 'completed' });
      await h.drain(); await h.drain();
      // 缺口帧被拒：不投影、不退休、水位不前进，且经真实 WS 请求补发。
      expect(streamBufferManager.isRunActive(PATH)).toBe(true);
      expect(getSessionStreamMeta(PATH)?.lastSeq).toBe(base + 1);
      expect(h.received.some(e => e.type === 'stream_resume')).toBe(true);
      // 补发 sinceSeq=base+1 的 (base+2) delta 与 (base+3) run_end（真实 stream_resume 帧格式）。
      h.inject({
        type: 'stream_resume', sessionPath: PATH, sessionId: ID, streamId: S,
        sinceSeq: base + 1, nextSeq: base + 4, isStreaming: false, runtimeIsStreaming: false,
        reset: false, truncated: false,
        events: [
          { seq: base + 2, event: { type: 'assistant_segment_delta', segmentId: 'seg-r1', delta: 'R1正文' } },
          { seq: base + 3, event: { type: 'assistant_run_end', runId: R, status: 'completed' } },
        ],
      });
      await h.drain(); await h.drain();
      // 正文完整、结束恰一次、消费者真实收尾。
      await vi.waitFor(() => { expect(projection()).toContain('R1正文'); });
      expect(streamBufferManager.isRunActive(PATH)).toBe(false);
      expect(useStore.getState().streamingSessions).not.toContain(ID);
      // 水位不虚增：被拒帧不得进入 consumedSeqs；已接纳帧都有投影证据。
      const meta = getSessionStreamMeta(PATH)!;
      expect(meta.lastSeq).toBe(base + 3);
      expect(meta.consumedSeqs.has(base + 1)).toBe(true);
      expect(meta.consumedSeqs.has(base + 2)).toBe(true);
      expect(meta.consumedSeqs.has(base + 3)).toBe(true);
      // 合法幂等重复 run_end：不重渲染、不二次收尾、水位不再前进。
      h.inject({ ...runStart, type: 'assistant_run_end', seq: base + 3, status: 'completed' });
      await h.drain();
      expect(streamBufferManager.isRunActive(PATH)).toBe(false);
      expect(getSessionStreamMeta(PATH)!.lastSeq).toBe(base + 3);
      expect(projection().split('R1正文').length - 1).toBe(1);
    } finally { await h.close(); }
  });

  it('a real server-store refill after a dropped window still lets the end frame finalize exactly once', async () => {
    const h = await harness();
    try {
      h.emit({ type: 'agent_start' }); await h.drain();
      const runStart = h.received.find(e => e.type === 'assistant_run_start')!;
      // 传输丢失：run(base) start 与 seg(base+1) start 送达后，其余真实帧全部丢失。
      const dropped: Event[] = [];
      h.setDrop(e => {
        if (e.type === 'assistant_run_start' || e.type === 'assistant_segment_start') return false;
        dropped.push(e);
        return true;
      });
      h.model('R1真链路正文');
      await h.drain();
      const seqs = dropped.map(e => Number(e.seq)).filter(Number.isFinite);
      expect(seqs.length).toBeGreaterThan(0);
      const maxDroppedSeq = Math.max(...seqs);
      h.setDrop(null);
      const S = runStart.streamId as string;
      // 缺口期间 run_end 先到（真实身份、服务器下一个 seq）。
      h.inject({ ...runStart, type: 'assistant_run_end', seq: maxDroppedSeq + 1, status: 'completed' });
      await h.drain(); await h.drain();
      // 真实服务器存储补发丢失窗口（对 run_end 缺口触发的 resume_stream 的真实响应）。
      const refill = h.received.find(e => e.type === 'stream_resume');
      expect(refill).toBeTruthy();
      await vi.waitFor(() => { expect(projection()).toContain('R1真链路正文'); });
      // 补发后重发结束帧：必须被接纳并恰一次收尾。
      h.inject({ ...runStart, type: 'assistant_run_end', seq: maxDroppedSeq + 1, status: 'completed' });
      await h.drain();
      expect(streamBufferManager.isRunActive(PATH)).toBe(false);
      expect(getSessionStreamMeta(PATH)!.streamId).toBe(S);
      expect(getSessionStreamMeta(PATH)!.lastSeq).toBe(maxDroppedSeq + 1);
    } finally { await h.close(); }
  });

  it('a gap-rejected end of a NEW stream does not retire the new run across the switch', async () => {
    const h = await harness();
    try {
      // 第一条流完整结束（真实链路），建立旧流退休集合。
      h.emit({ type: 'agent_start' }); h.model('第一流'); h.emit({ type: 'agent_settled' }); await h.drain();
      const firstEnd = h.received.filter(e => e.type === 'assistant_run_end').at(-1)!;
      const firstStream = firstEnd.streamId as string;
      // 新流开始（真实链路）。
      h.emit({ type: 'agent_start' }); await h.drain();
      const secondStart = h.received.filter(e => e.type === 'assistant_run_start').at(-1)!;
      const S2 = secondStart.streamId as string;
      const R2 = secondStart.runId as string;
      expect(S2).not.toBe(firstStream);
      const base = Number(secondStart.seq);
      h.inject({ ...secondStart, type: 'assistant_segment_start', seq: base + 1, segmentId: 'seg-r1c', kind: 'text' });
      // 新流 run_end 带缺口先到：被拒且不得退休新 run。
      h.inject({ ...secondStart, type: 'assistant_run_end', seq: base + 3, status: 'completed' });
      await h.drain(); await h.drain();
      expect(getSessionStreamMeta(PATH)?.streamId).toBe(S2);
      // 补发 (base+2) delta + (base+3) run_end。
      h.inject({
        type: 'stream_resume', sessionPath: PATH, sessionId: ID, streamId: S2,
        sinceSeq: base + 1, nextSeq: base + 4, isStreaming: false, runtimeIsStreaming: false,
        reset: false, truncated: false,
        events: [
          { seq: base + 2, event: { type: 'assistant_segment_delta', segmentId: 'seg-r1c', delta: '第二流结束' } },
          { seq: base + 3, event: { type: 'assistant_run_end', runId: R2, status: 'completed' } },
        ],
      });
      await h.drain(); await h.drain();
      await vi.waitFor(() => { expect(projection()).toContain('第二流结束'); });
      expect(streamBufferManager.isRunActive(PATH)).toBe(false);
      expect(getSessionStreamMeta(PATH)?.lastSeq).toBe(base + 3);
    } finally { await h.close(); }
  });
});

// ── R2：旧异步恢复不能覆盖等待期间接纳的新流（recheck F03）──

describe('R2 restore generations over the real route', () => {
  it('a background rebuild with a delayed history read cannot overwrite a newly admitted live stream', async () => {
    const h = await harness();
    try {
      focusOtherSession();
      // 旧 reset 恢复（受控延迟的历史读取）。
      historyGate.arm();
      h.inject({
        type: 'stream_resume', sessionPath: PATH, sessionId: ID, streamId: 'stream_old_reset',
        sinceSeq: 0, nextSeq: 3, isStreaming: true, reset: true, truncated: false,
        events: [
          { seq: 1, event: { type: 'assistant_run_start', runId: 'run_old' } },
          { seq: 2, event: { type: 'text_delta', delta: '旧流不得回流' } },
        ],
      });
      await vi.waitFor(() => { expect(historyGate.pending()).toBe(1); });
      // 等待期间经真实 WS 的新 run/stream 被接纳。
      h.emit({ type: 'agent_start' }); await h.drain();
      const newRun = h.received.find(e => e.type === 'assistant_run_start')!;
      expect(newRun.streamId).toBeTruthy();
      // 旧读取完成：新流必须继续是权威，旧恢复不写身份/消息/终态。
      historyGate.release();
      await h.drain(); await h.drain();
      const meta = getSessionStreamMeta(PATH)!;
      expect(meta.streamId).toBe(newRun.streamId);
      expect(meta.admission.isRetiredStream(newRun.streamId)).toBe(false);
      expect(streamBufferManager.isRunActive(PATH)).toBe(true);
      expect(projection()).not.toContain('旧流不得回流');
      // 后续新正文继续被接纳。
      h.inject({ ...newRun, type: 'assistant_segment_start', seq: 2, segmentId: 'seg-r2', kind: 'text' });
      h.inject({ ...newRun, type: 'assistant_segment_delta', seq: 3, segmentId: 'seg-r2', delta: '新流继续接纳' });
      await h.drain();
      await vi.waitFor(() => { expect(projection()).toContain('新流继续接纳'); });
      expect(getSessionStreamMeta(PATH)!.streamId).toBe(newRun.streamId);
    } finally { historyGate.release(); await h.close(); }
  });

  it('a late resume response for a superseded stream is dropped by the request generation correlation', async () => {
    const h = await harness();
    try {
      // 第一条流的所有实时帧在传输中全部丢失（客户端从未建立其身份）。
      const held: Event[] = [];
      let firstStreamId: string | null = null;
      h.setDrop(e => {
        if (e.type === 'stream_resume') { held.push(e); return true; }
        if (!firstStreamId && typeof e.streamId === 'string') firstStreamId = e.streamId;
        return !!firstStreamId && e.streamId === firstStreamId;
      });
      h.emit({ type: 'agent_start' }); h.model('S1正文'); h.emit({ type: 'agent_settled' });
      await h.drain();
      // 真实 resume_stream 请求（携带恢复代次 token），服务器真实响应被扣在客户端边界。
      requestStreamResume(PATH, { sessionId: ID });
      await vi.waitFor(() => { expect(held.length).toBe(1); });
      expect(held[0].streamId).toBe(firstStreamId);
      // 新流经真实 WS 开始并被接纳。
      h.setDrop(null);
      h.emit({ type: 'agent_start' }); await h.drain();
      const newRun = h.received.filter(e => e.type === 'assistant_run_start').at(-1)!;
      expect(newRun.streamId).not.toBe(firstStreamId);
      // 旧响应迟到：必须被恢复代次关联丢弃，不得覆盖新流权威。
      h.inject(held[0]);
      await h.drain(); await h.drain();
      expect(getSessionStreamMeta(PATH)?.streamId).toBe(newRun.streamId);
      expect(getSessionStreamMeta(PATH)?.admission.isRetiredStream(newRun.streamId as string)).toBe(false);
      h.inject({ ...newRun, type: 'assistant_segment_start', seq: 2, segmentId: 'seg-r2b', kind: 'text' });
      h.inject({ ...newRun, type: 'assistant_segment_delta', seq: 3, segmentId: 'seg-r2b', delta: '代次后新流' });
      await h.drain();
      await vi.waitFor(() => { expect(projection()).toContain('代次后新流'); });
      expect(projection()).not.toContain('S1正文');
    } finally { await h.close(); }
  });

  it('with two concurrent restores only the latest rebuild generation lands', async () => {
    const h = await harness();
    try {
      focusOtherSession();
      historyGate.arm();
      h.inject({
        type: 'stream_resume', sessionPath: PATH, sessionId: ID, streamId: 'stream_restore_gen1',
        sinceSeq: 0, nextSeq: 2, isStreaming: true, reset: true, truncated: false,
        events: [{ seq: 1, event: { type: 'text_delta', delta: '恢复一代正文' } }],
      });
      await vi.waitFor(() => { expect(historyGate.pending()).toBe(1); });
      h.inject({
        type: 'stream_resume', sessionPath: PATH, sessionId: ID, streamId: 'stream_restore_gen2',
        sinceSeq: 0, nextSeq: 2, isStreaming: true, reset: true, truncated: false,
        events: [{ seq: 1, event: { type: 'text_delta', delta: '恢复二代正文' } }],
      });
      await vi.waitFor(() => { expect(historyGate.pending()).toBe(2); });
      historyGate.release();
      await h.drain(); await h.drain();
      await vi.waitFor(() => { expect(projection()).toContain('恢复二代正文'); });
      expect(projection()).not.toContain('恢复一代正文');
      expect(getSessionStreamMeta(PATH)?.streamId).toBe('stream_restore_gen2');
    } finally { historyGate.release(); await h.close(); }
  });

  it('a current-session rebuild that loses focus during its delayed read abandons the restore and clears the rebuild gate', async () => {
    const h = await harness();
    try {
      useStore.setState({ currentSessionId: ID, currentSessionPath: PATH } as never);
      historyGate.arm();
      h.inject({
        type: 'stream_resume', sessionPath: PATH, sessionId: ID, streamId: 'stream_focus_reset',
        sinceSeq: 0, nextSeq: 2, isStreaming: true, reset: true, truncated: false,
        events: [{ seq: 1, event: { type: 'text_delta', delta: '焦点切换后不得回流' } }],
      });
      await vi.waitFor(() => { expect(historyGate.pending()).toBe(1); });
      expect(isStreamResumeRebuilding()).toBe(PATH);
      focusOtherSession();
      historyGate.release();
      await h.drain(); await h.drain();
      expect(isStreamResumeRebuilding()).toBe(null);
      expect(projection()).not.toContain('焦点切换后不得回流');
      // 会话回到后台后，其新帧继续被接纳。
      h.emit({ type: 'agent_start' }); await h.drain();
      const run = h.received.filter(e => e.type === 'assistant_run_start').at(-1)!;
      h.inject({ ...run, type: 'assistant_segment_start', seq: Number(run.seq) + 1, segmentId: 'seg-r2d', kind: 'text' });
      h.inject({ ...run, type: 'assistant_segment_delta', seq: Number(run.seq) + 2, segmentId: 'seg-r2d', delta: '后台恢复后新流' });
      await h.drain();
      await vi.waitFor(() => { expect(projection()).toContain('后台恢复后新流'); });
    } finally { historyGate.release(); await h.close(); }
  });
});
