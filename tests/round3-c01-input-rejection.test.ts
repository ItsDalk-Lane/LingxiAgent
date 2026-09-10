// @vitest-environment jsdom

/**
 * C01：服务端「接受前明确拒绝」的类型化结算闭环。
 *
 * 三层覆盖：
 * 1. 服务端路由层——input_rejected 回执形状（模型切换/会话忙/无效审阅 Agent/
 *    提交层标记错误），以及「已接受后运行失败不发拒绝回执」的反向断言。
 * 2. 提交层标记——真实 SDK fixture 验证 canonical 接受边界：busy 拒绝被标记，
 *    append 之后的供应商错误不被标记。
 * 3. 客户端结算——真实 ws-message-handler → coordinator → store 链路：看门狗
 *    前后、迟到尝试、幂等、跨会话/服务器/版本隔离、队列恢复顺序、屏障隔离。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

const previewRefreshMocks = vi.hoisted(() => ({
  changeOptions: { retryMissing: true, retryUnchanged: true },
  refreshOpenPreviewDocumentsForResourceChange: vi.fn(async () => undefined),
  markDeskTreeDirtyForResourceChange: vi.fn(),
}));

vi.mock('../desktop/src/react/hooks/use-stream-buffer', () => ({
  streamBufferManager: { handle: vi.fn(), beginRun: vi.fn(), finishRun: vi.fn() },
}));

vi.mock('../desktop/src/react/stores/session-actions', async (original) => ({
  ...await original<typeof import('../desktop/src/react/stores/session-actions')>(),
  loadSessions: vi.fn(), upsertOptimisticSessionFirstMessage: vi.fn(),
}));

vi.mock('../desktop/src/react/stores/channel-actions', () => ({
  loadChannels: vi.fn(),
  openChannel: vi.fn(),
}));

vi.mock('../desktop/src/react/stores/preview-actions', () => ({
  handleLegacyArtifactBlock: vi.fn(),
}));

vi.mock('../desktop/src/react/services/app-event-actions', () => ({
  handleAppEvent: vi.fn(),
}));

vi.mock('../desktop/src/react/utils/preview-document-refresh', () => ({
  PREVIEW_DOCUMENT_CHANGE_REFRESH_OPTIONS: previewRefreshMocks.changeOptions,
  refreshOpenPreviewDocumentsForResourceChange: previewRefreshMocks.refreshOpenPreviewDocumentsForResourceChange,
  markDeskTreeDirtyForResourceChange: previewRefreshMocks.markDeskTreeDirtyForResourceChange,
}));

vi.mock('../desktop/src/react/services/stream-resume', () => ({
  replayStreamResume: vi.fn(),
  isStreamResumeRebuilding: () => null,
  isStreamScopedMessage: () => false,
  updateSessionStreamMeta: vi.fn(),
}));

vi.mock('../desktop/src/react/services/stream-key-dispatcher', () => ({
  dispatchStreamKey: vi.fn(),
}));

const wsMocks = vi.hoisted(() => ({
  current: null as { send: Mock; readyState: number } | null,
}));

vi.mock('../desktop/src/react/services/websocket', () => ({
  getWebSocket: () => wsMocks.current,
}));

import { createChatRoute } from '../server/routes/chat.ts';
import {
  abortPendingDesktopSubmission,
  isDesktopInputRejectedBeforeAcceptance,
  markDesktopInputRejectedBeforeAcceptance,
  submitDesktopSessionMessage,
} from '../core/desktop-session-submit.ts';
import { AgentReviewTurnCoordinator } from '../lib/agent-review/turn-coordinator.ts';
import { useStore } from '../desktop/src/react/stores';
import { sessionScopedValue } from '../desktop/src/react/stores/session-slice';
import { handleServerMessage } from '../desktop/src/react/services/ws-message-handler';
import {
  discardRejectedUserMessage,
  getSendRecord,
  hasInFlightSend,
  noteComposerInputRejected,
  retryRejectedUserMessage,
  resetComposerSendCoordinatorForTests,
  retrySendRecord,
  sendWithLease,
  tryAcquireSendLease,
  type ComposerSessionIdentity,
} from '../desktop/src/react/services/composer-send-coordinator';
import type { ChatListItem, ComposerSendBundle, QueuedTurnInput } from '../desktop/src/react/stores/chat-types';
import { createTestTranslator } from '../desktop/src/react/__tests__/helpers/i18n-test-strings';
import { createDesktopInputHistoryFixture } from './helpers/desktop-input-history-fixture';

const testT = createTestTranslator();
const PATH_A = '/session/a.jsonl';
const PATH_B = '/session/b.jsonl';
const SESSION_A = 'sess-a';

function makeBundle(sessionPath: string, text: string, overrides: Partial<ComposerSendBundle> = {}): ComposerSendBundle {
  return {
    type: 'prompt',
    sessionRef: { sessionId: SESSION_A, sessionPath, agentId: 'hana' },
    text,
    skills: [],
    fileRefs: [],
    sessionRefs: [],
    agentMentions: [],
    inputFiles: [],
    knowledgeRefs: null,
    docContextAttached: false,
    doc: null,
    quotes: [],
    uiContext: null,
    ...overrides,
  };
}

function identityOf(sessionPath: string, sessionId = SESSION_A): ComposerSessionIdentity {
  return { kind: 'session', sessionId, sessionPath, agentId: 'hana' };
}

function seedStore(sessionPaths: string[]) {
  useStore.setState({
    currentSessionPath: sessionPaths[0] ?? null,
    currentSessionId: sessionPaths[0] ? SESSION_A : null,
    currentAgentId: 'hana',
    currentTab: 'chat',
    sessions: sessionPaths.map(path => ({ path, sessionId: SESSION_A, agentId: 'hana', agentName: 'Hana' })),
    sessionLocatorsById: { [SESSION_A]: { path: PATH_A } },
    connected: true,
    streamingSessions: [],
    activeSessionStreams: {},
    turnPendingSessions: [],
    knowledgeRetrievingSessions: [],
    knowledgeRollupBySession: {},
    knowledgeSupplementBySession: {},
    queuedTurnInputsByPath: {},
    capabilityRefreshingSessions: [],
    compactingSessions: [],
    compactionModeBySession: {},
    pendingSessionSwitchPath: null,
    modelSwitching: false,
    models: [{ id: 'm-text', provider: 'test', name: 'Text', input: ['text'], isCurrent: true }],
    sessionModelsByPath: {},
    toasts: [],
    inlineErrors: {},
    chatSessions: {},
    drafts: {},
    composerRevisionsByKey: {},
    attachedFiles: [],
    pendingNewSession: false,
    pendingDraftId: null,
  } as never);
}

function makeDeps() {
  return { loadVisionAuxiliaryConfig: vi.fn(async () => ({ enabled: false, model: null })), t: testT };
}

function messageData(item: ChatListItem | undefined) {
  if (!item || item.type !== 'message') throw new Error('expected message item');
  return item.data;
}

function itemsOf(sessionPath: string) {
  const state = useStore.getState();
  return (sessionScopedValue(state as never, state.chatSessions, sessionPath) || { items: [] }).items;
}

function queueOf(sessionPath: string): QueuedTurnInput[] {
  const state = useStore.getState();
  return sessionScopedValue(state as never, state.queuedTurnInputsByPath, sessionPath) || [];
}

function sentPayloads(): Array<Record<string, any>> {
  return wsMocks.current!.send.mock.calls.map(call => JSON.parse(String(call[0])));
}

/** 客户端完整提交一次输入（真实 coordinator → ws.send），返回记录与载荷。 */
async function submitViaCoordinator(text: string, sessionPath = PATH_A) {
  const identity = identityOf(sessionPath);
  const lease = tryAcquireSendLease({ identity, bundle: makeBundle(sessionPath, text) });
  if (lease.ok === false) {
    const why: string = lease.reason;
    throw new Error(`lease not acquired: ${why}`);
  }
  const result = await sendWithLease(lease.leaseId, makeDeps());
  if (result.kind !== 'transport_submitted') throw new Error(`not submitted: ${JSON.stringify(result)}`);
  const payload = sentPayloads().at(-1)!;
  const record = getSendRecord(lease.leaseId)!;
  return { record, payload, leaseId: lease.leaseId };
}

function dispatchRejection(input: {
  sessionPath?: string; sessionId?: string; clientMessageId: string; snapshotVersion?: number;
  clientAttemptId?: string | null; code?: string; retryable?: boolean;
}) {
  handleServerMessage({
    type: 'input_rejected',
    outcome: 'not_accepted',
    sessionPath: input.sessionPath ?? PATH_A,
    sessionId: input.sessionId ?? SESSION_A,
    clientMessageId: input.clientMessageId,
    snapshotVersion: input.snapshotVersion ?? 1,
    ...(input.clientAttemptId !== undefined ? { clientAttemptId: input.clientAttemptId } : {}),
    code: input.code ?? 'model_switching',
    message: 'synthetic rejection',
    retryable: input.retryable ?? true,
  });
}

// ── 服务端路由 harness ──

interface RouteHarness {
  handlers: any;
  ws: { readyState: number; send: Mock };
  sent: () => Array<Record<string, any>>;
  receive: (msg: Record<string, unknown>) => void;
  engine: Record<string, any>;
  hub: Record<string, any>;
}

function buildChatRouteHarness(engineOverrides: Record<string, any> = {}, hubOverrides: Record<string, any> = {}): RouteHarness {
  let createHandlers: any;
  const upgradeWebSocket = vi.fn((factory: any) => { createHandlers = factory; return () => new Response(null); });
  let subscriber: ((event: any, sessionPath: string) => void) | undefined;
  const hub = {
    subscribe: vi.fn((fn: any) => { subscriber = fn; }),
    send: vi.fn(async () => { throw new Error('hub.send not expected'); }),
    eventBus: { emit: vi.fn() },
    abort: vi.fn(async () => false),
    ...hubOverrides,
  };
  const engine: Record<string, any> = {
    agentName: 'Hana',
    abortAllStreaming: vi.fn(async () => {}),
    getSessionByPath: vi.fn(() => ({ entries: [], sessionManager: { getBranch: () => [] } })),
    isSessionStreaming: vi.fn(() => false),
    isSessionSwitching: vi.fn(() => false),
    steerSession: vi.fn(() => false),
    slashDispatcher: null,
    getSessionIdForPath: vi.fn((p: string) => (p === PATH_A ? SESSION_A : null)),
    getSessionManifest: vi.fn(() => ({ currentLocator: { path: PATH_A }, ownerAgentId: 'hana' })),
    resolveSessionOwnership: vi.fn(() => ({ agentId: 'hana', agentDeleted: false })),
    isDeletedAgent: vi.fn(() => false),
    terminalSessions: {},
    getRuntimeContext: vi.fn(() => ({})),
    getAgent: vi.fn(() => ({ agentName: 'Hana' })),
    emitEvent: vi.fn(),
    ...engineOverrides,
  };
  createChatRoute(engine as any, hub as any, { upgradeWebSocket });
  const handlers = createHandlers({});
  const ws = { readyState: 1, send: vi.fn() };
  handlers.onOpen({}, ws);
  return {
    handlers,
    ws: ws as any,
    engine,
    hub,
    sent: () => ws.send.mock.calls.map(([raw]: [string]) => JSON.parse(raw)),
    receive: (msg: Record<string, unknown>) => { handlers.onMessage({ data: JSON.stringify(msg) }, ws); },
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  wsMocks.current = { send: vi.fn(), readyState: 1 };
  resetComposerSendCoordinatorForTests();
  seedStore([PATH_A]);
  window.platform = { readFileBase64: vi.fn(async () => 'SUJBTkVfQkFTRTY0') } as unknown as typeof window.platform;
});

afterEach(() => {
  resetComposerSendCoordinatorForTests();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  delete (window as { platform?: unknown }).platform;
});

// ── 服务端路由层：input_rejected 回执形状 ──

describe('C01 server route: typed input_rejected receipts', () => {
  it('模型切换拒绝：error + input_rejected（not_accepted / code / retryable / 身份与尝试回显）', async () => {
    const h = buildChatRouteHarness({ isSessionSwitching: () => true });
    h.receive({ type: 'prompt', text: 'hi', sessionId: SESSION_A, sessionPath: PATH_A, clientMessageId: 'client-user-1', snapshotVersion: 2, clientAttemptId: 'client-attempt-1' });
    await vi.runAllTimersAsync();

    const payloads = h.sent();
    const legacy = payloads.find(p => p.type === 'error');
    expect(legacy).toMatchObject({ sessionPath: PATH_A });
    const rejected = payloads.find(p => p.type === 'input_rejected');
    expect(rejected).toBeDefined();
    expect(rejected).toMatchObject({
      outcome: 'not_accepted',
      sessionId: SESSION_A,
      sessionPath: PATH_A,
      clientMessageId: 'client-user-1',
      snapshotVersion: 2,
      clientAttemptId: 'client-attempt-1',
      code: 'model_switching',
      retryable: true,
    });
    expect(h.hub.send).not.toHaveBeenCalled();
  });

  it('会话忙拒绝：code=session_busy 且 retryable=true', async () => {
    const h = buildChatRouteHarness({ isSessionStreaming: () => true });
    h.receive({ type: 'prompt', text: 'hi', sessionId: SESSION_A, sessionPath: PATH_A, clientMessageId: 'client-user-2', snapshotVersion: 1 });
    await vi.runAllTimersAsync();
    expect(h.sent().find(p => p.type === 'input_rejected')).toMatchObject({ code: 'session_busy', retryable: true, clientMessageId: 'client-user-2' });
  });

  it('无效审阅 Agent（评审者=属主）：code=invalid_review_agent 且 retryable=false', async () => {
    const h = buildChatRouteHarness({});
    h.receive({
      type: 'prompt', text: 'please review', sessionId: SESSION_A, sessionPath: PATH_A,
      clientMessageId: 'client-user-3', snapshotVersion: 1,
      agentReviewRequests: [{ agentId: 'hana', label: 'Hana' }],
    });
    await vi.runAllTimersAsync();
    expect(h.sent().find(p => p.type === 'input_rejected')).toMatchObject({
      code: 'invalid_review_agent',
      retryable: false,
      clientMessageId: 'client-user-3',
    });
  });

  it('已接受后的运行失败（hub.send 抛未标记错误）：不发 input_rejected，只保留普通 error', async () => {
    const providerError = new Error('provider 500');
    const h = buildChatRouteHarness({}, { send: vi.fn(async () => { throw providerError; }) });
    h.receive({ type: 'prompt', text: 'hi', sessionId: SESSION_A, sessionPath: PATH_A, clientMessageId: 'client-user-4', snapshotVersion: 1 });
    await vi.runAllTimersAsync();
    const payloads = h.sent();
    expect(payloads.some(p => p.type === 'error')).toBe(true);
    expect(payloads.some(p => p.type === 'input_rejected')).toBe(false);
  });

  it('hub.send 抛出提交层标记的「未接受」错误：发 input_rejected（code=session_busy）', async () => {
    const busyError = new Error('session_busy');
    markDesktopInputRejectedBeforeAcceptance(busyError);
    const h = buildChatRouteHarness({}, { send: vi.fn(async () => { throw busyError; }) });
    h.receive({ type: 'prompt', text: 'hi', sessionId: SESSION_A, sessionPath: PATH_A, clientMessageId: 'client-user-5', snapshotVersion: 1, clientAttemptId: 'client-attempt-5' });
    await vi.runAllTimersAsync();
    expect(h.sent().find(p => p.type === 'input_rejected')).toMatchObject({
      code: 'session_busy',
      retryable: true,
      clientAttemptId: 'client-attempt-5',
    });
  });

  it('已证实接受前取消：发送拒绝回执并恢复客户端输入，不额外显示普通错误', async () => {
    const { record, payload } = await submitViaCoordinator('取消后原文仍可处置');
    const loaded = Promise.withResolvers<object>();
    const promptSession = vi.fn();
    const h = buildChatRouteHarness({ ensureSessionLoaded: () => loaded.promise, promptSession }, {
      send: vi.fn((text, opts) => submitDesktopSessionMessage(h.engine, { ...opts, text })),
    });
    h.receive(payload);
    await vi.advanceTimersByTimeAsync(0);
    expect(abortPendingDesktopSubmission(h.engine, { sessionId: SESSION_A, sessionPath: PATH_A })).toBe(true);
    loaded.resolve({});
    await vi.advanceTimersByTimeAsync(0);
    const rejection = h.sent().find(event => event.type === 'input_rejected');
    expect(rejection).toMatchObject({ outcome: 'not_accepted', code: 'input_cancelled_before_acceptance', retryable: true });
    expect(h.sent().some(event => event.type === 'error')).toBe(false);
    handleServerMessage(rejection);
    expect(record.phase).toBe('rejected_before_acceptance');
    expect(record.bundle.text).toBe('取消后原文仍可处置');
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(false);
    expect(promptSession).not.toHaveBeenCalled();
  });

  it('旧 steer 路由等待异步结果，false 才回退到普通发送', async () => {
    const enqueue = Promise.withResolvers<boolean>();
    const h = buildChatRouteHarness({ steerSession: () => enqueue.promise }, { send: vi.fn(async () => {}) });
    h.receive({ type: 'steer', text: 'hi', sessionId: SESSION_A, sessionPath: PATH_A });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.sent().some(event => event.type === 'steered')).toBe(false);
    expect(h.hub.send).not.toHaveBeenCalled();
    enqueue.resolve(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(h.hub.send).toHaveBeenCalledOnce();
  });

  it('无 clientMessageId 的旧客户端：只收到 legacy error，不投递拒绝回执', async () => {
    const h = buildChatRouteHarness({ isSessionSwitching: () => true });
    h.receive({ type: 'prompt', text: 'hi', sessionId: SESSION_A, sessionPath: PATH_A });
    await vi.runAllTimersAsync();
    const payloads = h.sent();
    expect(payloads.some(p => p.type === 'error')).toBe(true);
    expect(payloads.some(p => p.type === 'input_rejected')).toBe(false);
  });
});

// ── 提交层标记：canonical 接受边界 ──

describe('C01 submit layer: not-accepted evidence vs canonical acceptance', () => {
  it('busy 门禁拒绝被标记为未接受', async () => {
    const engine: any = {
      ensureSessionLoaded: vi.fn(async () => ({})),
      promptSession: vi.fn(async () => { throw new Error('should not reach'); }),
      isSessionStreaming: () => true,
      getSessionManifest: () => ({ currentLocator: { path: PATH_A } }),
    };
    let caught: any;
    await submitDesktopSessionMessage(engine, { sessionId: SESSION_A, text: 'hi', clientMessageId: 'c1' } as any).catch(err => { caught = err; });
    expect(caught?.message).toBe('session_busy');
    expect(isDesktopInputRejectedBeforeAcceptance(caught)).toBe(true);
  });

  it('真实 SDK：canonical append 之后的供应商错误不被标记为未接受', async () => {
    vi.useRealTimers();
    const { createAssistantMessageEventStream } = await import('@earendil-works/pi-ai');
    const f = await createDesktopInputHistoryFixture();
    // 第一轮成功提交，确认 canonical 关联链路健康。
    await f.submit('client-user-seed', 1);
    const seeded = f.manager.getBranch().filter(entry => entry.type === 'message' && entry.message?.role === 'user');
    expect(seeded.length).toBeGreaterThanOrEqual(1);
    // 第二轮：canonical append 之后的提交链故障。真实供应商流错误会被 SDK 化解成
    // run 级事件（不 reject 提交）；这里让「回合结束的 session_status:false 广播」
    // 在第二轮抛错——它发生在 promptSession 正常返回之后（canonical 已触发），
    // 错误会沿提交 Promise 冒出，是「已接受后的运行失败」的确定性注入点。
    const originalEmit = f.engine.emitEvent;
    let armPostAcceptanceFailure = false;
    let canonicalSeenWhenArmed = false;
    f.engine.emitEvent = vi.fn((event: any, sp: string) => {
      if (armPostAcceptanceFailure && event?.type === 'session_user_message' && event?.message?.sourceEntryId) {
        canonicalSeenWhenArmed = true;
        return undefined;
      }
      if (armPostAcceptanceFailure && canonicalSeenWhenArmed && event?.type === 'session_status' && event?.isStreaming === false) {
        throw new Error('synthetic post-acceptance sink failure');
      }
      return originalEmit?.call(null, event, sp);
    });
    armPostAcceptanceFailure = true;
    let caught: any;
    await f.submit('client-user-boom', 1).catch(err => { caught = err; });
    expect(caught).toBeTruthy();
    // 用户消息已经 append（canonical 接受证据存在）→ 错误不得标记为未接受。
    expect(canonicalSeenWhenArmed).toBe(true);
    const boomUsers = f.manager.getBranch().filter(entry => entry.type === 'message' && entry.message?.role === 'user');
    expect(boomUsers.length).toBeGreaterThanOrEqual(2);
    expect(isDesktopInputRejectedBeforeAcceptance(caught)).toBe(false);
  });

  it('真实 SDK：流式中的 busy 拒绝被标记，用户消息未落盘', async () => {
    vi.useRealTimers();
    const { createAssistantMessageEventStream } = await import('@earendil-works/pi-ai');
    const f = await createDesktopInputHistoryFixture();
    // 第一轮挂起（流永不结束）占住 session：isSessionStreaming=true。
    let held!: ReturnType<typeof createAssistantMessageEventStream>;
    f.stream.mockImplementationOnce(() => {
      held = createAssistantMessageEventStream();
      return held;
    });
    const first = f.submit('client-user-hold', 1).catch(() => {});
    await new Promise(resolve => setTimeout(resolve, 100));
    let caught: any;
    await f.submit('client-user-busy', 1).catch(err => { caught = err; });
    expect(caught?.message).toBe('session_busy');
    expect(isDesktopInputRejectedBeforeAcceptance(caught)).toBe(true);
    // busy 输入没有落盘：用户消息仍只有第一轮 hold 的一条。
    const users = f.manager.getBranch().filter(entry => entry.type === 'message' && entry.message?.role === 'user');
    expect(users).toHaveLength(1);
    // 释放挂起的流，让 fixture 清理可以完成。
    held.push({ type: 'done', reason: 'stop', message: { role: 'assistant', content: [{ type: 'text', text: 'release' }], api: 'openai-completions', provider: 'synthetic-r04', model: 'synthetic-r04-model', usage: { input: 1, output: 1, cacheRead: 0, cacheWrite: 0, totalTokens: 2, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: 'stop', timestamp: Date.now() } } as any);
    held.end();
    await first;
  });
});

// ── 评审协调器：onRejected 结算通知 ──

describe('C01 agent review: onRejected notification', () => {
  beforeEach(() => { vi.useRealTimers(); });

  function buildCoordinator(overrides: Record<string, any> = {}) {
    const statuses: any[] = [];
    const engine: any = {
      emitEvent: vi.fn(),
      getSessionByPath: () => ({ model: { id: 'm' } }),
      getSessionManifest: () => ({}),
      createDetachedSession: vi.fn(async () => ({ sessionId: 'rev-1', sessionPath: '/rev.jsonl' })),
      abortSession: vi.fn(async () => {}),
      ...overrides,
    };
    const coordinator = new AgentReviewTurnCoordinator({
      engine,
      submitSessionMessage: vi.fn(async () => ({ text: 'review text' })),
      emitStatus: (status) => { statuses.push(status); },
    });
    return { coordinator, engine, statuses };
  }

  const baseInput = {
    reviewedSessionId: SESSION_A,
    reviewedSessionPath: PATH_A,
    reviewer: { agentId: 'reviewer' },
    text: 'please review',
    clientMessageId: 'client-user-rev',
    snapshotVersion: 1,
  };

  it('父会话提交前失败（模型缺失）通知 onRejected，父会话从未收到提交', async () => {
    const { coordinator } = buildCoordinator({ getSessionByPath: () => null });
    const rejections: any[] = [];
    await coordinator.start({ ...baseInput, onRejected: info => rejections.push(info) });
    expect(rejections).toHaveLength(1);
    expect(rejections[0]).toMatchObject({ code: 'reviewed_session_model_unavailable', retryable: true });
  });

  it('评审完成（父会话提交成功）不通知 onRejected', async () => {
    const { coordinator } = buildCoordinator();
    const rejections: any[] = [];
    await coordinator.start({ ...baseInput, onRejected: info => rejections.push(info) });
    expect(rejections).toHaveLength(0);
  });

  it('用户在父会话提交前停止：通知 onRejected（review_cancelled_before_acceptance）', async () => {
    let releaseReviewer!: () => void;
    const reviewerGate = new Promise<void>(resolve => { releaseReviewer = resolve; });
    const { coordinator } = buildCoordinator({
      createDetachedSession: vi.fn(async () => {
        await reviewerGate;
        return { sessionId: 'rev-1', sessionPath: '/rev.jsonl' };
      }),
    });
    const rejections: any[] = [];
    const start = coordinator.start({ ...baseInput, onRejected: info => rejections.push(info) });
    await new Promise(resolve => setTimeout(resolve, 20));
    await coordinator.cancelByParent(SESSION_A, 'user_abort');
    releaseReviewer();
    await start;
    expect(rejections).toHaveLength(1);
    expect(rejections[0]).toMatchObject({ code: 'review_cancelled_before_acceptance', retryable: true });
  });
});

// ── 客户端结算：真实 handler → coordinator → store ──

describe('C01 client settlement via real ws-message-handler', () => {
  it('模型切换拒绝：结算为明确失败、释放屏障、保留快照，修正后可显式重试（路由全链路）', async () => {
    const h = buildChatRouteHarness({ isSessionSwitching: () => true });
    const { record, payload } = await submitViaCoordinator('待发消息');
    expect(record.phase).toBe('awaiting_ack');

    // 客户端载荷经真实 ws 路由提交到服务端；服务端回包（error + input_rejected）
    // 再经真实 handler 进入 store/coordinator——完整闭环。
    h.receive(payload);
    await vi.advanceTimersByTimeAsync(0);
    const rejectedFromServer = h.sent().find(p => p.type === 'input_rejected');
    expect(rejectedFromServer).toMatchObject({ clientMessageId: record.clientMessageId });
    for (const out of h.sent()) handleServerMessage(out);

    expect(record.phase).toBe('rejected_before_acceptance');
    expect(record.code).toBe('model_switching');
    expect(record.retryable).toBe(true);
    expect(record.bundle.text).toBe('待发消息');
    // 屏障释放：同会话可再次取得租约（transport_busy 消失）。
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(false);
    // 消息投影：明确失败 + 可重试，正文原样保留。
    const item = itemsOf(PATH_A).find(i => i.type === 'message' && i.data.id === record.clientMessageId);
    const data = messageData(item);
    expect(data.sendStatus).toBe('failed');
    expect(data.sendError).toBe('model_switching');
    expect(data.sendRetryable).toBe(true);
    expect(data.text).toBe('待发消息');

    // 修正（切换完成）后显式重试：同一逻辑身份、新尝试身份、不产生重复消息项。
    h.engine.isSessionSwitching = () => false;
    h.hub.send = vi.fn(async () => ({}));
    const retry = await retryRejectedUserMessage(PATH_A, record.clientMessageId, makeDeps());
    expect(retry?.kind).toBe('transport_submitted');
    expect(record.phase).toBe('awaiting_ack');
    const retryPayload = sentPayloads().at(-1)!;
    expect(retryPayload.clientMessageId).toBe(record.clientMessageId);
    expect(retryPayload.clientAttemptId).not.toBe(payload.clientAttemptId);
    const matches = itemsOf(PATH_A).filter(i => i.type === 'message' && i.data.id === record.clientMessageId);
    expect(matches).toHaveLength(1);
    // 重试把失败投影重置回「等待回执」的 pending 态。
    expect(messageData(matches[0]).sendStatus).toBe('pending');
  });

  it('负回执在看门狗超时之前到达：不触发 delivery_unknown，也不启动对账', async () => {
    const { record } = await submitViaCoordinator('watchdog before');
    dispatchRejection({ clientMessageId: record.clientMessageId, clientAttemptId: record.activeAttemptId });
    expect(record.phase).toBe('rejected_before_acceptance');
    vi.advanceTimersByTime(20_000);
    expect(record.phase).toBe('rejected_before_acceptance');
    expect(messageData(itemsOf(PATH_A).find(i => i.type === 'message' && i.data.id === record.clientMessageId)).sendError)
      .toBe('model_switching');
  });

  it('负回执在看门狗超时之后到达：delivery_unknown 被确定拒绝覆盖', async () => {
    const { record } = await submitViaCoordinator('watchdog after');
    vi.advanceTimersByTime(16_000);
    expect(record.phase).toBe('delivery_unknown');
    dispatchRejection({ clientMessageId: record.clientMessageId, clientAttemptId: record.activeAttemptId });
    expect(record.phase).toBe('rejected_before_acceptance');
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(false);
  });

  it('第一尝试迟到负回执不影响第二尝试；重复回执幂等', async () => {
    const { record } = await submitViaCoordinator('attempt one');
    const firstAttemptId = record.activeAttemptId!;
    dispatchRejection({ clientMessageId: record.clientMessageId, clientAttemptId: firstAttemptId });
    expect(record.phase).toBe('rejected_before_acceptance');
    // 显式重试 → 新尝试。
    const retry = await retrySendRecord(record.leaseId, makeDeps());
    expect(retry?.kind).toBe('transport_submitted');
    expect(record.phase).toBe('awaiting_ack');
    const secondAttemptId = record.activeAttemptId!;
    expect(secondAttemptId).not.toBe(firstAttemptId);
    // 第一次尝试的迟到负回执：不结算第二次尝试。
    dispatchRejection({ clientMessageId: record.clientMessageId, clientAttemptId: firstAttemptId, code: 'session_busy' });
    expect(record.phase).toBe('awaiting_ack');
    // 重复的当前尝试回执：幂等，不改变状态。
    dispatchRejection({ clientMessageId: record.clientMessageId, clientAttemptId: secondAttemptId });
    expect(record.phase).toBe('rejected_before_acceptance');
    expect(record.code).toBe('model_switching');
  });

  it('跨会话 / 跨服务器 / 错误版本回执无效果', async () => {
    const { record } = await submitViaCoordinator('isolation');
    // 跨会话。
    dispatchRejection({ clientMessageId: record.clientMessageId, sessionPath: PATH_B, sessionId: 'sess-b', clientAttemptId: record.activeAttemptId });
    expect(record.phase).toBe('awaiting_ack');
    // 跨服务器（不同 originConnectionKey）。
    const cross = noteComposerInputRejected({
      originConnectionKey: JSON.stringify(['other-server']),
      sessionId: SESSION_A, sessionPath: PATH_A,
      clientMessageId: record.clientMessageId, snapshotVersion: 1,
      clientAttemptId: record.activeAttemptId,
    }, { code: 'model_switching', retryable: true });
    expect(cross).toBe(false);
    expect(record.phase).toBe('awaiting_ack');
    // 错误版本。
    dispatchRejection({ clientMessageId: record.clientMessageId, snapshotVersion: 9, clientAttemptId: record.activeAttemptId });
    expect(record.phase).toBe('awaiting_ack');
  });

  it('已有 canonical 接受证据时，冲突负回执不倒退为未接收', async () => {
    const { record } = await submitViaCoordinator('accepted conflict');
    handleServerMessage({
      type: 'session_user_message',
      sessionPath: PATH_A, sessionId: SESSION_A,
      clientMessageId: record.clientMessageId, snapshotVersion: 1,
      message: { id: 'srv-entry-1', text: 'accepted conflict', timestamp: new Date().toISOString() },
    });
    expect(record.acceptance).toBe('accepted');
    dispatchRejection({ clientMessageId: record.clientMessageId, clientAttemptId: record.activeAttemptId });
    expect(record.acceptance).toBe('accepted');
    expect(record.phase).toBe('accepted');
  });

  it('已接受后的供应商错误（普通 error）：不结算为未接收、不自动重发', async () => {
    const { record } = await submitViaCoordinator('provider error later');
    handleServerMessage({
      type: 'session_user_message',
      sessionPath: PATH_A, sessionId: SESSION_A,
      clientMessageId: record.clientMessageId, snapshotVersion: 1,
      message: { id: 'srv-entry-2', text: 'provider error later', timestamp: new Date().toISOString() },
    });
    handleServerMessage({ type: 'error', sessionPath: PATH_A, message: 'provider 500' });
    expect(record.acceptance).toBe('accepted');
    expect(record.phase).toBe('accepted');
    const sendsBefore = sentPayloads().length;
    vi.advanceTimersByTime(30_000);
    expect(sentPayloads().length).toBe(sendsBefore);
    const data = messageData(itemsOf(PATH_A).find(i => i.type === 'message' && i.data.id === record.clientMessageId));
    expect(data.sendStatus).toBeUndefined();
  });

  it('拒绝项的正文/附件/引用/知识范围完整保留在失败投影', async () => {
    const bundle = makeBundle(PATH_A, '正文内容', {
      quotes: [{ text: '被引用的段落', start: 0, end: 8 } as never],
      knowledgeRefs: { notebookIds: ['nb-1'], mode: 'fast', notebookNames: { 'nb-1': '笔记本' } } as never,
    });
    const identity = identityOf(PATH_A);
    const acquired = tryAcquireSendLease({ identity, bundle });
    if (!acquired.ok) throw new Error('lease not acquired');
    const result = await sendWithLease(acquired.leaseId, makeDeps());
    expect(result.kind).toBe('transport_submitted');
    const record = getSendRecord(acquired.leaseId)!;
    dispatchRejection({ clientMessageId: record.clientMessageId, clientAttemptId: record.activeAttemptId });
    const data = messageData(itemsOf(PATH_A).find(i => i.type === 'message' && i.data.id === record.clientMessageId));
    expect(data.text).toBe('正文内容');
    expect(data.quotedText).toContain('被引用的段落');
    expect(data.knowledgeRefs?.notebookIds).toEqual(['nb-1']);
  });

  it('A 被拒（transport_submitted 后出队）恢复原位失败；不跳过 A 发 B；处置 A 后 B 推进', async () => {
    // 队列：A、B。
    const itemA: QueuedTurnInput = { id: 'qa', sessionPath: PATH_A, createdAt: 1, text: 'A 消息', bundle: makeBundle(PATH_A, 'A 消息'), snapshotVersion: 1 };
    const itemB: QueuedTurnInput = { id: 'qb', sessionPath: PATH_A, createdAt: 2, text: 'B 消息', bundle: makeBundle(PATH_A, 'B 消息'), snapshotVersion: 1 };
    useStore.getState().enqueueQueuedTurnInput(PATH_A, itemA);
    useStore.getState().enqueueQueuedTurnInput(PATH_A, itemB);
    const { dispatchQueuedItem, flushQueuedHeadNow, findSendRecordByClientMessageId } = await import('../desktop/src/react/services/composer-send-coordinator');
    const dispatched = await dispatchQueuedItem(itemA, { type: 'prompt' }, makeDeps());
    expect(dispatched.kind).toBe('transport_submitted');
    // transport_submitted 已把 A 出队（快照移交乐观消息）。
    expect(queueOf(PATH_A).map(item => item.id)).toEqual(['qb']);
    const record = findSendRecordByClientMessageId(String(sentPayloads().at(-1)!.clientMessageId))!;
    expect(record.queueItemId).toBe('qa');

    dispatchRejection({ clientMessageId: record.clientMessageId, clientAttemptId: record.activeAttemptId });

    // A 原位恢复为失败输入（顺序保持队首），B 不越过 A。
    const queue = queueOf(PATH_A);
    expect(queue.map(item => item.id)).toEqual(['qa', 'qb']);
    expect(queue[0]).toMatchObject({ status: 'failed', errorCode: 'model_switching', retryable: true });

    const sendsBefore = sentPayloads().length;
    await flushQueuedHeadNow(PATH_A, makeDeps());
    expect(sentPayloads().length).toBe(sendsBefore); // B 未被发出

    // 处置 A（明确放弃）后 B 正常推进。
    expect(discardRejectedUserMessage(PATH_A, record.clientMessageId)).toBe(true);
    expect(queueOf(PATH_A).map(item => item.id)).toEqual(['qb']);
    useStore.setState({ currentSessionPath: PATH_A, currentSessionId: SESSION_A } as never);
    await flushQueuedHeadNow(PATH_A, makeDeps());
    expect(sentPayloads().length).toBe(sendsBefore + 1);
    expect(sentPayloads().at(-1)!.text).toContain('B 消息');
  });

  it('真正的 delivery_unknown 屏障仍有效，且不阻塞其他会话', async () => {
    const { record } = await submitViaCoordinator('unknown barrier');
    vi.advanceTimersByTime(16_000);
    expect(record.phase).toBe('delivery_unknown');
    // 该会话新输入被未决屏障拦下。
    const blocked = tryAcquireSendLease({ identity: identityOf(PATH_A), bundle: makeBundle(PATH_A, '第二条') });
    expect(blocked).toMatchObject({ ok: false, reason: 'transport_busy' });
    // 其他会话不受影响。
    const other = tryAcquireSendLease({ identity: identityOf(PATH_B, 'sess-b'), bundle: makeBundle(PATH_B, '其他会话') });
    expect(other.ok).toBe(true);
  });
});
