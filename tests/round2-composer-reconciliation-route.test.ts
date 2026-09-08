const historyMocks = vi.hoisted(() => ({ fetch: vi.fn() }));
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

vi.mock('../desktop/src/react/hooks/use-stream-buffer', () => ({
  streamBufferManager: {
    handle: vi.fn(),
    beginRun: vi.fn(),
    finishRun: vi.fn(),
  },
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
  // 每个用例重建；readyState 1 = OPEN。
  current: null as { send: Mock; readyState: number } | null,
}));

vi.mock('../desktop/src/react/services/websocket', () => ({
  getWebSocket: () => wsMocks.current,
}));

import { useStore } from '../desktop/src/react/stores';
import { sessionScopedValue } from '../desktop/src/react/stores/session-slice';
import { handleServerMessage } from '../desktop/src/react/services/ws-message-handler';
import {
  beginQueuedItemEdit,
  cancelQueuedItemEdit,
  saveQueuedItemEdit,
  cancelQueueFlushIntent,
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
} from '../desktop/src/react/services/composer-send-coordinator';
import type { ChatListItem, ComposerSendBundle, QueuedTurnInput } from '../desktop/src/react/stores/chat-types';
import { createTestTranslator } from '../desktop/src/react/__tests__/helpers/i18n-test-strings';

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
    currentTab: 'chat',
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
    snapshotVersion: 1,
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
  historyMocks.fetch.mockReset();
  useStore.setState({activeServerConnection:createLocalServerConnection({serverPort:18799,serverToken:"synthetic"})});
  window.platform = { readFileBase64: vi.fn(async () => 'SUJBTkVfQkFTRTY0') } as unknown as typeof window.platform;
});

afterEach(() => {
  resetComposerSendCoordinatorForTests();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  delete (window as { platform?: unknown }).platform;
});


import * as coordinator from '../desktop/src/react/services/composer-send-coordinator';
import { createLocalServerConnection } from '../desktop/src/react/services/server-connection';


import { createDesktopInputHistoryFixture } from './helpers/desktop-input-history-fixture';

it('R04-01/02/15：真实SDK提交后丢失ACK，断线恢复经真实history路由合并并续发一次', async()=>{
  vi.useRealTimers();
  const f=await createDesktopInputHistoryFixture();
  const connection=createLocalServerConnection({serverPort:18799,serverToken:'synthetic-auth'})!;
  useStore.setState({activeServerConnection:connection,currentSessionPath:f.sessionPath,currentSessionId:f.sessionId,
    sessions:[{path:f.sessionPath,sessionId:f.sessionId,agentId:'test-agent',agentName:'Synthetic'} as never],
    sessionLocatorsById:{[f.sessionId]:{path:f.sessionPath}},chatSessions:{},queuedTurnInputsByPath:{},turnPendingSessions:[],streamingSessions:[]});
  const identity:ComposerSessionIdentity={kind:'session',sessionId:f.sessionId,sessionPath:f.sessionPath,agentId:'test-agent'};
  const bundle={...makeBundle(f.sessionPath,'原始用户消息  \n'),sessionRef:{sessionId:f.sessionId,sessionPath:f.sessionPath,agentId:'test-agent'}};
  let submitted:Promise<unknown>=Promise.resolve();
  wsMocks.current!.send.mockImplementation((raw:string)=>{const input=JSON.parse(raw);submitted=f.submit(input.clientMessageId,input.snapshotVersion);});
  const acquired=tryAcquireSendLease({identity,bundle});if(!acquired.ok)throw new Error('lease');
  expect((await sendWithLease(acquired.leaseId,makeDeps())).kind).toBe('transport_submitted');
  await submitted;
  // 服务端真实执行并落盘，但此客户端没有消费任何ACK或run事件。
  const record=getSendRecord(acquired.leaseId)!;expect(record.phase).toBe('awaiting_ack');
  noteComposerConnectionClosed();noteComposerConnectionOpened();
  const queued:QueuedTurnInput={id:'next',sessionPath:f.sessionPath,createdAt:Date.now(),text:'第二条',bundle:{...bundle,text:'第二条'}};
  useStore.getState().enqueueQueuedTurnInput(f.sessionPath,queued);
  requestQueueFlush(f.sessionPath,makeDeps());
  await new Promise(resolve=>setTimeout(resolve,450));
  expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
  const fetch=vi.fn(async(input:RequestInfo|URL,options?:RequestInit)=>{
    const url=new URL(String(input));
    expect(url.origin).toBe(connection.baseUrl);
    return f.app.request(url.pathname+url.search,options);
  });vi.stubGlobal('fetch',fetch);
  await coordinator.reconcileComposerSession(f.sessionPath);
  expect(record.acceptance).toBe('accepted');expect(record.runStatus).toBe('reconciled_idle');
  expect(hasInFlightSend(identity)).toBe(false);expect(fetch).toHaveBeenCalledTimes(1);
  const state=useStore.getState();const items=sessionScopedValue(state,state.chatSessions,f.sessionPath)!.items;
  expect(items).toHaveLength(1);expect(messageData(items[0]).text).toBe(bundle.text);
  const users=f.manager.getBranch().filter(entry=>entry.type==='message'&&entry.message.role==='user');
  expect(messageData(items[0]).sourceEntryId).toBe(users[0].id);
  await new Promise(resolve=>setTimeout(resolve,450));
  await submitted;
  expect(wsMocks.current!.send).toHaveBeenCalledTimes(2);
});
