// @vitest-environment jsdom

/**
 * composer-send-coordinator 测试（F1/F2 · Q01–Q16）。
 *
 * 覆盖任务书 P3.5 最低测试集：会话级发送租约（preparing/awaiting_ack）、
 * 回执关联、连接代次、队列续发串行、立即插入门禁、编辑/删除取消语义。
 * 全部使用可控 Promise 与 fake timers，不用真实 sleep。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

const previewRefreshMocks = vi.hoisted(() => ({
  changeOptions: { retryMissing: true, retryUnchanged: true },
  refreshOpenPreviewDocumentsForResourceChange: vi.fn(async () => undefined),
  markDeskTreeDirtyForResourceChange: vi.fn(),
}));

vi.mock('../../hooks/use-stream-buffer', () => ({
  streamBufferManager: {
    handle: vi.fn(),
    beginRun: vi.fn(),
    finishRun: vi.fn(),
  },
}));

vi.mock('../../stores/session-actions', () => ({
  loadSessions: vi.fn(),
  upsertOptimisticSessionFirstMessage: vi.fn(),
}));

vi.mock('../../stores/channel-actions', () => ({
  loadChannels: vi.fn(),
  openChannel: vi.fn(),
}));

vi.mock('../../stores/preview-actions', () => ({
  handleLegacyArtifactBlock: vi.fn(),
}));

vi.mock('../../services/app-event-actions', () => ({
  handleAppEvent: vi.fn(),
}));

vi.mock('../../utils/preview-document-refresh', () => ({
  PREVIEW_DOCUMENT_CHANGE_REFRESH_OPTIONS: previewRefreshMocks.changeOptions,
  refreshOpenPreviewDocumentsForResourceChange: previewRefreshMocks.refreshOpenPreviewDocumentsForResourceChange,
  markDeskTreeDirtyForResourceChange: previewRefreshMocks.markDeskTreeDirtyForResourceChange,
}));

vi.mock('../../services/stream-resume', () => ({
  replayStreamResume: vi.fn(),
  isStreamResumeRebuilding: () => null,
  isStreamScopedMessage: () => false,
  updateSessionStreamMeta: vi.fn(),
}));

vi.mock('../../services/stream-key-dispatcher', () => ({
  dispatchStreamKey: vi.fn(),
}));

const wsMocks = vi.hoisted(() => ({
  // 每个用例重建；readyState 1 = OPEN。
  current: null as { send: Mock; readyState: number } | null,
}));

vi.mock('../../services/websocket', () => ({
  getWebSocket: () => wsMocks.current,
}));

import { useStore } from '../../stores';
import { sessionScopedValue } from '../../stores/session-slice';
import { handleServerMessage } from '../../services/ws-message-handler';
import {
  cancelQueueItemSend,
  dispatchQueuedItem,
  findSendRecordByClientMessageId,
  flushQueuedHeadNow,
  getComposerConnectionGeneration,
  getSendRecord,
  hasInFlightSend,
  noteComposerConnectionClosed,
  noteComposerConnectionOpened,
  requestQueueFlush,
  resetComposerSendCoordinatorForTests,
  resolveQueuedInsertNowAction,
  sendWithLease,
  tryAcquireSendLease,
  type ComposerSessionIdentity,
} from '../../services/composer-send-coordinator';
import type { ChatListItem, ComposerSendBundle, QueuedTurnInput } from '../../stores/chat-types';
import { createTestTranslator } from '../helpers/i18n-test-strings';

const testT = createTestTranslator();

const PATH_A = '/session/a.jsonl';
const PATH_B = '/session/b.jsonl';

function makeBundle(sessionPath: string, text: string, overrides: Partial<ComposerSendBundle> = {}): ComposerSendBundle {
  return {
    type: 'prompt',
    sessionRef: { sessionId: `sess-${sessionPath}`, sessionPath, agentId: 'hana' },
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

function identityOf(sessionPath: string): ComposerSessionIdentity {
  return { kind: 'session', sessionId: `sess-${sessionPath}`, sessionPath, agentId: 'hana' };
}

function seedStore(sessionPaths: string[]) {
  useStore.setState({
    currentSessionPath: sessionPaths[0] ?? null,
    currentSessionId: sessionPaths[0] ? `sess-${sessionPaths[0]}` : null,
    currentAgentId: 'hana',
    sessions: sessionPaths.map(path => ({
      path,
      sessionId: `sess-${path}`,
      agentId: 'hana',
      agentName: 'Hana',
    })),
    sessionLocatorsById: Object.fromEntries(sessionPaths.map(path => [`sess-${path}`, { path }])),
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
    models: [{ id: 'm-vision', provider: 'test', name: 'Vision', input: ['text', 'image'], isCurrent: true }],
    sessionModelsByPath: {},
    toasts: [],
    inlineErrors: {},
    chatSessions: {},
    drafts: {},
    draftDocs: {},
    composerRevisionsByKey: {},
    attachedFiles: [],
    attachedFilesBySession: {},
    quotedSelections: [],
    quotedSelection: null,
    quoteCandidate: null,
    docContextAttached: false,
    pendingNewSession: false,
    pendingDraftId: null,
  } as never);
}

function makeDeps() {
  return {
    loadVisionAuxiliaryConfig: vi.fn(async () => ({ enabled: false, model: null })),
    t: testT,
  };
}

function enqueue(sessionPath: string, id: string, text: string, overrides: Partial<ComposerSendBundle> = {}) {
  const item: QueuedTurnInput = {
    id,
    sessionPath,
    text,
    createdAt: Date.now(),
    bundle: makeBundle(sessionPath, text, overrides),
  };
  useStore.getState().enqueueQueuedTurnInput(sessionPath, item);
  return item;
}

function queueOf(sessionPath: string): QueuedTurnInput[] {
  const state = useStore.getState();
  return sessionScopedValue(state as never, state.queuedTurnInputsByPath, sessionPath) || [];
}

/** 取消息项的 data（ChatListItem 是联合类型，先收窄到 message）。 */
function messageData(item: ChatListItem | undefined) {
  if (!item || item.type !== 'message') throw new Error('expected message item');
  return item.data;
}
function sentPayloads(): Array<Record<string, unknown>> {
  return wsMocks.current!.send.mock.calls.map(call => JSON.parse(String(call[0])));
}

function dispatchAck(sessionPath: string, clientMessageId: string, text = 'acked') {
  handleServerMessage({
    type: 'session_user_message',
    sessionPath,
    sessionId: `sess-${sessionPath}`,
    clientMessageId,
    message: { id: `srv-${clientMessageId}`, text, timestamp: new Date().toISOString() },
  });
}

function dispatchStatus(sessionPath: string, isStreaming: boolean, streamId: string | null) {
  handleServerMessage({ type: 'status', sessionPath, isStreaming, streamId, turnId: null });
}

function makeGatedRead() {
  let release!: (value: string) => void;
  const gate = new Promise<string>((resolve) => { release = resolve; });
  const readFileBase64 = vi.fn(() => gate);
  window.platform = { readFileBase64 } as unknown as typeof window.platform;
  return { release, readFileBase64 };
}

const IMAGE_FILE = { fileId: 'sf_img', path: '/tmp/a.png', name: 'a.png', isDirectory: false };

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

describe('composer-send-coordinator（Q01–Q16）', () => {
  it('Q01：队首附件读取挂起时，后续纯文本项不准备、不先发', async () => {
    const gated = makeGatedRead();
    enqueue(PATH_A, 'q-a', 'A', { inputFiles: [IMAGE_FILE] });
    enqueue(PATH_A, 'q-b', 'B');

    const flushA = flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(0);
    expect(gated.readFileBase64).toHaveBeenCalledTimes(1);

    // 再次触发 flush（effect 重跑/其他会话调度）：B 不得插队准备或发送。
    await flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(1000);
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    expect(queueOf(PATH_A).map(item => item.id)).toEqual(['q-a', 'q-b']);

    gated.release('SU1HX0E=');
    await flushA;
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    expect(sentPayloads()[0].text).toBe('A');
    // A 已发出但回执/回合未终结：B 仍在队列中等待。
    expect(queueOf(PATH_A).map(item => item.id)).toEqual(['q-b']);
  });

  it('Q02：A 已发送但回执未到时，传输租约阻止 B 发送', async () => {
    enqueue(PATH_A, 'q-a', 'A');
    enqueue(PATH_A, 'q-b', 'B');

    await flushQueuedHeadNow(PATH_A, makeDeps());
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(true);

    // 隔离验证租约门禁：即使 turnPending 被其他原因清掉，awaiting_ack 租约在，B 也不发。
    useStore.getState().endTurnPending(PATH_A);
    await flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(1000);
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);

    // 回执到达（租约释放）后 B 才可以发。
    const clientMessageId = String(sentPayloads()[0].clientMessageId);
    dispatchAck(PATH_A, clientMessageId);
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(false);
    await flushQueuedHeadNow(PATH_A, makeDeps());
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(2);
    expect(sentPayloads()[1].text).toBe('B');
  });

  it('Q03：A 回执到但 run 未结束（含 run 进行中），B 均不发', async () => {
    enqueue(PATH_A, 'q-a', 'A');
    enqueue(PATH_A, 'q-b', 'B');

    await flushQueuedHeadNow(PATH_A, makeDeps());
    const clientMessageId = String(sentPayloads()[0].clientMessageId);

    // 回执到达：租约释放，但「等待助手」pending 不得被回执清除（否则此处 B 会抢跑）。
    dispatchAck(PATH_A, clientMessageId);
    expect(findSendRecordByClientMessageId(clientMessageId)?.phase).toBe('accepted');
    expect(useStore.getState().turnPendingSessions.length).toBeGreaterThan(0);

    await flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(1000);
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);

    // run 开始：pending 清除、streaming 置位，B 仍然不发。
    dispatchStatus(PATH_A, true, 'run-1');
    expect(useStore.getState().turnPendingSessions).toEqual([]);
    await flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(1000);
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);

    // run 权威终结后 B 才发出。
    dispatchStatus(PATH_A, false, 'run-1');
    await flushQueuedHeadNow(PATH_A, makeDeps());
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(2);
    expect(sentPayloads()[1].text).toBe('B');
  });

  it('Q04：model_turn_end / tool_end / mood_end 均不释放下一轮', async () => {
    enqueue(PATH_A, 'q-a', 'A');
    enqueue(PATH_A, 'q-b', 'B');

    await flushQueuedHeadNow(PATH_A, makeDeps());
    const clientMessageId = String(sentPayloads()[0].clientMessageId);
    dispatchAck(PATH_A, clientMessageId);
    dispatchStatus(PATH_A, true, 'run-1');
    expect(useStore.getState().streamingSessions.length).toBe(1);

    for (const type of ['model_turn_end', 'tool_end', 'mood_end']) {
      handleServerMessage({ type, sessionPath: PATH_A, streamId: 'run-1', id: `t-${type}` });
      expect(useStore.getState().streamingSessions.length).toBe(1);
      await flushQueuedHeadNow(PATH_A, makeDeps());
      await vi.advanceTimersByTimeAsync(100);
      expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    }
    expect(queueOf(PATH_A).map(item => item.id)).toEqual(['q-b']);
  });

  it('Q05：A 正确 run_end 后，B 只发一次且顺序稳定（经 400ms 调度通道）', async () => {
    // A 正在 run：队列积压 A→B（流式期间发送入队的真实形态）。
    dispatchStatus(PATH_A, true, 'run-0');
    enqueue(PATH_A, 'q-a', 'A');
    enqueue(PATH_A, 'q-b', 'B');

    // run-0 结束 → 组件 effect 发出调度意图（400ms 视觉缓冲后重新验证并取得租约）。
    dispatchStatus(PATH_A, false, 'run-0');
    requestQueueFlush(PATH_A, makeDeps());
    requestQueueFlush(PATH_A, makeDeps()); // 重复 effect 触发去重
    await vi.advanceTimersByTimeAsync(400);
    await vi.advanceTimersByTimeAsync(0);
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    expect(sentPayloads()[0].text).toBe('A');

    const clientMessageId = String(sentPayloads()[0].clientMessageId);
    dispatchAck(PATH_A, clientMessageId);
    dispatchStatus(PATH_A, true, 'run-1');
    dispatchStatus(PATH_A, false, 'run-1');

    requestQueueFlush(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(400);
    await vi.advanceTimersByTimeAsync(0);
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(2);
    expect(sentPayloads()[1].text).toBe('B');
    expect(queueOf(PATH_A)).toEqual([]);
  });

  it('Q06：重复 effect / StrictMode 双调用同一调度，不重复提交', async () => {
    enqueue(PATH_A, 'q-a', 'A');

    // 双调用防抖通道 + 双调用直通道并发：租约同步占用，只有一份发送。
    requestQueueFlush(PATH_A, makeDeps());
    requestQueueFlush(PATH_A, makeDeps());
    const direct1 = flushQueuedHeadNow(PATH_A, makeDeps());
    const direct2 = flushQueuedHeadNow(PATH_A, makeDeps());
    await Promise.all([direct1, direct2]);
    await vi.advanceTimersByTimeAsync(1000);
    await vi.advanceTimersByTimeAsync(0);

    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    expect(queueOf(PATH_A)).toEqual([]);
  });

  it('Q07：准备 A 时手动发送 C，C 不超越 B', async () => {
    const gated = makeGatedRead();
    enqueue(PATH_A, 'q-a', 'A', { inputFiles: [IMAGE_FILE] });
    enqueue(PATH_A, 'q-b', 'B');

    const flushA = flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(0);
    expect(gated.readFileBase64).toHaveBeenCalledTimes(1);

    // 手动发送 C 时队列非空/有在途发送：coordinator 报告的占用状态驱动 InputArea 入队，
    // 且直接抢租约必然失败——两条路径 C 都拿不到发送权。
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(true);
    const steal = tryAcquireSendLease({ identity: identityOf(PATH_A), bundle: makeBundle(PATH_A, 'C') });
    expect(steal.ok).toBe(false);
    enqueue(PATH_A, 'q-c', 'C');

    // A 完成完整回合（回执 + run 起止）后，顺序必须是 A → B → C。
    gated.release('SU1HX0E=');
    await flushA;
    dispatchAck(PATH_A, String(sentPayloads()[0].clientMessageId));
    dispatchStatus(PATH_A, true, 'run-1');
    dispatchStatus(PATH_A, false, 'run-1');

    await flushQueuedHeadNow(PATH_A, makeDeps());
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(2);
    expect(sentPayloads()[1].text).toBe('B');

    dispatchAck(PATH_A, String(sentPayloads()[1].clientMessageId));
    dispatchStatus(PATH_A, true, 'run-2');
    dispatchStatus(PATH_A, false, 'run-2');
    await flushQueuedHeadNow(PATH_A, makeDeps());
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(3);
    expect(sentPayloads()[2].text).toBe('C');
  });

  it('Q08：A1 会话准备挂起，不阻塞空闲的 A2 会话', async () => {
    seedStore([PATH_A, PATH_B]);
    const gated = makeGatedRead();
    enqueue(PATH_A, 'q-a', 'A', { inputFiles: [IMAGE_FILE] });
    enqueue(PATH_B, 'q-b', 'B');

    const flushA = flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(0);
    expect(gated.readFileBase64).toHaveBeenCalledTimes(1);

    await flushQueuedHeadNow(PATH_B, makeDeps());
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    expect(sentPayloads()[0].sessionPath).toBe(PATH_B);

    gated.release('SU1HX0E=');
    await flushA;
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(2);
    expect(sentPayloads()[1].sessionPath).toBe(PATH_A);
  });

  it('Q09：准备期间连接代次切换，旧租约不得向新连接发送旧载荷', async () => {
    noteComposerConnectionOpened();
    const generation = getComposerConnectionGeneration();
    const gated = makeGatedRead();
    const acq = tryAcquireSendLease({
      identity: identityOf(PATH_A),
      bundle: makeBundle(PATH_A, 'A', { inputFiles: [IMAGE_FILE] }),
    });
    expect(acq.ok).toBe(true);
    if (!acq.ok) return;
    expect(getSendRecord(acq.leaseId)?.connectionGeneration).toBe(generation);

    const pending = sendWithLease(acq.leaseId, makeDeps());
    await vi.advanceTimersByTimeAsync(0);

    // 断线 → 重连（新 socket、新代次）。
    noteComposerConnectionClosed();
    noteComposerConnectionOpened();
    wsMocks.current = { send: vi.fn(), readyState: 1 };

    gated.release('SU1HX0E=');
    const result = await pending;
    expect(result).toEqual({ kind: 'failed_before_submit', code: 'connection_changed', retryable: true });
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    expect(getSendRecord(acq.leaseId)?.phase).toBe('failed_before_submit');
  });

  it('Q10：旧 run 的迟到终态不结束新 run', async () => {
    enqueue(PATH_A, 'q-a', 'A');
    enqueue(PATH_A, 'q-b', 'B');

    await flushQueuedHeadNow(PATH_A, makeDeps());
    dispatchAck(PATH_A, String(sentPayloads()[0].clientMessageId));
    dispatchStatus(PATH_A, true, 'run-2');

    // 旧 run-1 的迟到 status(false)：身份不匹配，不得结束 run-2。
    dispatchStatus(PATH_A, false, 'run-1');
    expect(useStore.getState().streamingSessions.length).toBe(1);
    await flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(100);
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);

    // 匹配的 run-2 终态才释放下一轮；重复的终态事件是 no-op。
    dispatchStatus(PATH_A, false, 'run-2');
    dispatchStatus(PATH_A, false, 'run-2');
    expect(useStore.getState().streamingSessions).toEqual([]);
    await flushQueuedHeadNow(PATH_A, makeDeps());
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(2);
    expect(sentPayloads()[1].text).toBe('B');
  });

  it('Q11：编辑/删除正在准备的队列项，过期快照不得发送', async () => {
    const gated = makeGatedRead();
    const itemA = enqueue(PATH_A, 'q-a', 'A', { inputFiles: [IMAGE_FILE] });

    const flushA = flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(0);
    expect(gated.readFileBase64).toHaveBeenCalledTimes(1);

    // 编辑保存 = 取消在途准备 + 更新文本（快照版本递增）。
    cancelQueueItemSend(itemA.id);
    useStore.getState().updateQueuedTurnInputText(PATH_A, itemA.id, 'A-edited');

    gated.release('SU1HX0E=');
    await flushA;
    await vi.advanceTimersByTimeAsync(0);
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    const afterEdit = queueOf(PATH_A);
    expect(afterEdit).toHaveLength(1);
    expect(afterEdit[0].text).toBe('A-edited');
    expect(afterEdit[0].snapshotVersion).toBe(2);
    expect(afterEdit[0].status ?? 'ready').toBe('ready');

    // 编辑路径收尾：让编辑后的 q-a 正常发出并收到回执，腾空传输租约与
    // 回合门禁（awaiting_ack 租约不回执不放行下一项），再进入删除路径。
    window.platform = { readFileBase64: vi.fn(async () => 'SU1HX0E=') } as unknown as typeof window.platform;
    await flushQueuedHeadNow(PATH_A, makeDeps());
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    expect(sentPayloads()[0].text).toBe('A-edited');
    expect(queueOf(PATH_A)).toEqual([]);
    dispatchAck(PATH_A, String(sentPayloads()[0].clientMessageId));
    useStore.getState().endTurnPending(PATH_A);
    wsMocks.current!.send.mockClear();

    // 删除路径：在途准备被取消后，迟到的读取结果同样不得发送。
    const gated2 = makeGatedRead();
    const itemB = enqueue(PATH_A, 'q-b', 'B', { inputFiles: [IMAGE_FILE] });
    const flushB = flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(0);
    expect(gated2.readFileBase64).toHaveBeenCalledTimes(1);
    cancelQueueItemSend(itemB.id);
    useStore.getState().removeQueuedTurnInput(PATH_A, itemB.id);
    gated2.release('SU1HX0I=');
    await flushB;
    await vi.advanceTimersByTimeAsync(0);
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    expect(queueOf(PATH_A)).toEqual([]);
  });

  it('Q12：立即插入与普通续发并发，不双发、不绕过预检', async () => {
    const gated = makeGatedRead();
    dispatchStatus(PATH_A, true, 'run-1');
    const itemX = enqueue(PATH_A, 'q-x', 'X', { inputFiles: [IMAGE_FILE] });
    enqueue(PATH_A, 'q-b', 'B');

    expect(resolveQueuedInsertNowAction(itemX, { streaming: true, turnPending: false })).toBe('interject');
    const insert = dispatchQueuedItem(itemX, { type: 'interject', targetRun: { streamId: 'run-1', turnId: null } }, makeDeps());
    await vi.advanceTimersByTimeAsync(0);

    // 插入在途期间的普通续发：租约占用，不得双发。
    await flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(100);
    expect(wsMocks.current!.send).not.toHaveBeenCalled();

    gated.release('SU1HX1g=');
    const result = await insert;
    expect(result.kind).toBe('transport_submitted');
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    expect(sentPayloads()[0].type).toBe('interject');
    expect(sentPayloads()[0].text).toBe('X');
    // 插入成功后队列项移除，后续 B 仍按序等待。
    expect(queueOf(PATH_A).map(item => item.id)).toEqual(['q-b']);

    // 回执释放传输租约，下一项才允许进入准备。
    dispatchAck(PATH_A, String(sentPayloads()[0].clientMessageId));

    // 预检不被插入路径绕过：超限视频明确 blocked，原位保留。
    const itemV = enqueue(PATH_A, 'q-v', 'V', {
      inputFiles: [1, 2, 3, 4].map(i => ({ fileId: `sf_v${i}`, path: `/tmp/v${i}.mp4`, name: `v${i}.mp4`, mimeType: 'video/mp4', isDirectory: false })),
    });
    const blockedResult = await dispatchQueuedItem(itemV, { type: 'interject', targetRun: { streamId: 'run-1', turnId: null } }, makeDeps());
    expect(blockedResult).toEqual({ kind: 'blocked', code: 'video_count_exceeded', retryable: false });
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    const kept = queueOf(PATH_A).find(item => item.id === 'q-v');
    expect(kept?.status).toBe('blocked');
    expect(kept?.errorCode).toBe('video_count_exceeded');
  });

  it('Q13：run 在插话准备期间结束/更换，不得插入错误的 run', async () => {
    // 场景一：run 更换（run-1 → run-2）。
    let gated = makeGatedRead();
    dispatchStatus(PATH_A, true, 'run-1');
    const itemX = enqueue(PATH_A, 'q-x', 'X', { inputFiles: [IMAGE_FILE] });
    const insert = dispatchQueuedItem(itemX, { type: 'interject', targetRun: { streamId: 'run-1', turnId: null } }, makeDeps());
    await vi.advanceTimersByTimeAsync(0);
    dispatchStatus(PATH_A, true, 'run-2');
    gated.release('SU1HX1g=');
    const changed = await insert;
    expect(changed).toEqual({ kind: 'blocked', code: 'run_changed', retryable: false });
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    expect(queueOf(PATH_A).find(item => item.id === 'q-x')?.status).toBe('blocked');

    // 场景二：run 直接结束。
    gated = makeGatedRead();
    useStore.setState({ streamingSessions: [], activeSessionStreams: {} } as never);
    dispatchStatus(PATH_A, true, 'run-3');
    const itemY = enqueue(PATH_A, 'q-y', 'Y', { inputFiles: [IMAGE_FILE] });
    const insert2 = dispatchQueuedItem(itemY, { type: 'interject', targetRun: { streamId: 'run-3', turnId: null } }, makeDeps());
    await vi.advanceTimersByTimeAsync(0);
    dispatchStatus(PATH_A, false, 'run-3');
    gated.release('SU1HX1k=');
    const ended = await insert2;
    expect(ended).toEqual({ kind: 'blocked', code: 'run_ended', retryable: false });
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
  });

  it('Q14：agent review 排队项立即插入保留既有禁止语义，不静默丢失', async () => {
    dispatchStatus(PATH_A, true, 'run-1');
    const item = enqueue(PATH_A, 'q-review', '请审一下', {
      agentMentions: [{ agentId: 'reviewer', label: 'Reviewer' }] as never,
    });

    expect(resolveQueuedInsertNowAction(item, { streaming: true, turnPending: false })).toBe('review_forbidden');
    // 禁止语义下组件不派发：队列项原样保留，无任何发送。
    await vi.advanceTimersByTimeAsync(1000);
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    expect(queueOf(PATH_A).map(queued => queued.id)).toEqual(['q-review']);
    // 空闲后按普通 prompt 派发是允许的（单 mention = 合法 review 请求）。
    expect(resolveQueuedInsertNowAction(item, { streaming: false, turnPending: false })).toBe('prompt');
    // awaiting_ack（无活跃 run）时：明确等待，不凭外观猜测。
    expect(resolveQueuedInsertNowAction(item, { streaming: false, turnPending: true })).toBe('wait');
  });

  it('Q15：abort 后先收到权威终态再续发；迟到媒体结果不重开旧 run', async () => {
    enqueue(PATH_A, 'q-a', 'A');
    enqueue(PATH_A, 'q-b', 'B');

    await flushQueuedHeadNow(PATH_A, makeDeps());
    dispatchAck(PATH_A, String(sentPayloads()[0].clientMessageId));
    dispatchStatus(PATH_A, true, 'run-1');

    // 用户点击停止：终态未到达前不得续发（不能点击停止就立刻清 busy）。
    await flushQueuedHeadNow(PATH_A, makeDeps());
    await vi.advanceTimersByTimeAsync(500);
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);

    // 权威终态到达。
    dispatchStatus(PATH_A, false, 'run-1');
    expect(useStore.getState().streamingSessions).toEqual([]);

    // 迟到的旧 run 媒体/过程事件：不得重开旧 run、不得触发额外发送。
    for (const type of ['text_delta', 'model_turn_end', 'tool_end']) {
      handleServerMessage({ type, sessionPath: PATH_A, streamId: 'run-1', id: `late-${type}` });
    }
    expect(useStore.getState().streamingSessions).toEqual([]);

    await flushQueuedHeadNow(PATH_A, makeDeps());
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(2);
    expect(sentPayloads()[1].text).toBe('B');
  });

  it('Q16：两个会话各自一次发送，投影与载荷互不混块', async () => {
    seedStore([PATH_A, PATH_B]);
    const acqA = tryAcquireSendLease({ identity: identityOf(PATH_A), bundle: makeBundle(PATH_A, '给A') });
    const acqB = tryAcquireSendLease({ identity: identityOf(PATH_B), bundle: makeBundle(PATH_B, '给B') });
    expect(acqA.ok && acqB.ok).toBe(true);
    if (!acqA.ok || !acqB.ok) return;

    const [resultA, resultB] = await Promise.all([
      sendWithLease(acqA.leaseId, makeDeps()),
      sendWithLease(acqB.leaseId, makeDeps()),
    ]);
    expect(resultA.kind).toBe('transport_submitted');
    expect(resultB.kind).toBe('transport_submitted');

    const payloads = sentPayloads();
    expect(payloads).toHaveLength(2);
    const forA = payloads.find(p => p.sessionPath === PATH_A)!;
    const forB = payloads.find(p => p.sessionPath === PATH_B)!;
    expect(forA.sessionId).toBe(`sess-${PATH_A}`);
    expect(forB.sessionId).toBe(`sess-${PATH_B}`);
    expect(forA.text).toBe('给A');
    expect(forB.text).toBe('给B');
    expect(forA.clientMessageId).not.toBe(forB.clientMessageId);

    const state = useStore.getState();
    const itemsA = sessionScopedValue(state as never, state.chatSessions, PATH_A)?.items || [];
    const itemsB = sessionScopedValue(state as never, state.chatSessions, PATH_B)?.items || [];
    expect(itemsA).toHaveLength(1);
    expect(itemsB).toHaveLength(1);
    expect(messageData(itemsA[0]).text).toBe('给A');
    expect(messageData(itemsB[0]).text).toBe('给B');
  });
});
