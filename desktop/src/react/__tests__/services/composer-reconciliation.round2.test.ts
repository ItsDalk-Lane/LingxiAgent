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

vi.mock('../../hooks/use-stream-buffer', () => ({
  streamBufferManager: {
    handle: vi.fn(),
    beginRun: vi.fn(),
    finishRun: vi.fn(),
  },
}));

vi.mock('../../stores/session-actions', () => ({
  fetchSessionHistoryPage: (...args: unknown[]) => historyMocks.fetch(...args),
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


import * as coordinator from '../../services/composer-send-coordinator';
import { createLocalServerConnection } from '../../services/server-connection';

describe('R04 未决输入与捕获连接对账', () => {
  async function submitted(path = PATH_A) {
    const acquired = tryAcquireSendLease({identity: identityOf(path), bundle: makeBundle(path, '原文  \n')});
    if (!acquired.ok) throw new Error('lease missing');
    await sendWithLease(acquired.leaseId, makeDeps());
    return getSendRecord(acquired.leaseId)!;
  }
  it('R04-14：断线未知保留独立barrier，A不能越过而B正常发送', async () => {
    seedStore([PATH_A, PATH_B]);
    const record = await submitted();
    noteComposerConnectionClosed();
    noteComposerConnectionOpened();
    expect(record.phase).toBe('delivery_unknown');
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(true);
    expect(tryAcquireSendLease({identity: identityOf(PATH_A),bundle:makeBundle(PATH_A,'下一条')})).toEqual({ok:false,reason:'transport_busy'});
    expect(tryAcquireSendLease({identity: identityOf(PATH_B),bundle:makeBundle(PATH_B,'B')} ).ok).toBe(true);
  });
  it('R04-13：超过200条历史记录不能淘汰未知快照', async () => {
    const first = await submitted();
    noteComposerConnectionClosed();
    for (let i=0;i<205;i++) {
      const path = '/session/other-'+i+'.jsonl';
      const lease = tryAcquireSendLease({identity:identityOf(path),bundle:makeBundle(path,'其他草稿')});
      if(lease.ok) coordinator.cancelSendLease(lease.leaseId);
    }
    expect(getSendRecord(first.leaseId)?.bundle.text).toBe('原文  \n');
  });

  it('R04-13：未决记录达到保护阈值后显式阻止新增，不删除任何未知快照', async () => {
    const paths = Array.from({ length: 512 }, (_, index) => `/session/unresolved-${index}.jsonl`);
    seedStore(paths);
    const leases: string[] = [];
    for (const path of paths) {
      const acquired = tryAcquireSendLease({ identity: identityOf(path), bundle: makeBundle(path, `消息-${path}`) });
      if (!acquired.ok) throw new Error('threshold setup lease missing');
      leases.push(acquired.leaseId);
      expect((await sendWithLease(acquired.leaseId, makeDeps())).kind).toBe('transport_submitted');
    }
    noteComposerConnectionClosed();
    const blocked = tryAcquireSendLease({
      identity: identityOf('/session/over-limit.jsonl'),
      bundle: makeBundle('/session/over-limit.jsonl', '保留为草稿'),
    });
    expect(blocked).toEqual({ ok: false, reason: 'unresolved_limit' });
    expect(leases.every(leaseId => getSendRecord(leaseId)?.phase === 'delivery_unknown')).toBe(true);
  });
  it('R04-11：canonical ACK仅确认接收，未证明运行结束仍保留barrier', async () => {
    const record = await submitted();
    dispatchAck(PATH_A, record.clientMessageId);
    expect(record.phase).toBe('accepted');
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(true);
  });
  function page(record: ReturnType<typeof getSendRecord>, runStatus = 'reconciled_idle', extra: Record<string,unknown> = {}) {
    return {messages:[{id:'0',role:'user',clientMessageId:record!.clientMessageId,sourceEntryId:'entry-a',snapshotVersion:record!.snapshotVersion}],hasMore:false,
      reconciliation:{sessionId:`sess-${PATH_A}`,sessionPath:PATH_A,complete:true,snapshotId:'query',runRevision:2,runStatus},...extra};
  }
  it('R04-02：精确历史命中和权威idle解除barrier，保留原文附件且不追加气泡', async()=>{
    const record=await submitted(); noteComposerConnectionClosed();
    historyMocks.fetch.mockResolvedValue(page(record));
    await coordinator.reconcileComposerSession(PATH_A);
    expect(record.acceptance).toBe('accepted'); expect(record.runStatus).toBe('reconciled_idle');
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(false);
    const state=useStore.getState();const items=sessionScopedValue(state,state.chatSessions,PATH_A)!.items;
    expect(items).toHaveLength(1); expect(messageData(items[0]).text).toBe('原文  \n');
    expect(messageData(items[0]).sourceEntryId).toBe('entry-a');
  });
  it('R04-03：命中但run running不释放下一条',async()=>{
    const record=await submitted();historyMocks.fetch.mockResolvedValue(page(record,'running'));
    await coordinator.reconcileComposerSession(PATH_A);
    expect(record.acceptance).toBe('accepted');expect(hasInFlightSend(identityOf(PATH_A))).toBe(true);
  });
  it('R04-04：沿before查第二页，第一页未命中不判拒收',async()=>{
    const record=await submitted();historyMocks.fetch.mockResolvedValueOnce(page(record,'unknown',{messages:[{id:'25',role:'assistant'}],hasMore:true})).mockResolvedValueOnce(page(record));
    await coordinator.reconcileComposerSession(PATH_A);
    expect(historyMocks.fetch).toHaveBeenCalledTimes(2);
    expect(historyMocks.fetch.mock.calls[1][2].before).toBe('25'); expect(record.acceptance).toBe('accepted');
  });
  it('R04-05：20页上限仍未命中保留unknown和完整快照',async()=>{
    const record=await submitted();let count=0;
    historyMocks.fetch.mockImplementation(async()=>page(record,'unknown',{messages:[{id:String(100-count++),role:'assistant'}],hasMore:true}));
    await coordinator.reconcileComposerSession(PATH_A);
    expect(historyMocks.fetch).toHaveBeenCalledTimes(20); expect(record.acceptance).toBe('unproven');
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(true);expect(record.bundle.text).toBe('原文  \n');
  });
  it('R04-06：同正文其他client或快照版本不提供接收证据',async()=>{
    const record=await submitted(); const response=page(record);
    response.messages[0].snapshotVersion=99; historyMocks.fetch.mockResolvedValue(response);
    await coordinator.reconcileComposerSession(PATH_A);expect(record.acceptance).toBe('unproven');
    response.messages[0].snapshotVersion=1;response.messages[0].clientMessageId='different';
    await coordinator.reconcileComposerSession(PATH_A);expect(record.acceptance).toBe('unproven');
  });
  it.each([403,404,'network'])('R04-12：读取%s保留barrier与诊断',async(code)=>{
    const record=await submitted();historyMocks.fetch.mockRejectedValue(new Error(String(code)));
    await coordinator.reconcileComposerSession(PATH_A);
    expect(record.code).toBe(String(code));expect(record.reconciliationStatus).toBe('failed');expect(hasInFlightSend(identityOf(PATH_A))).toBe(true);
  });
  it('R04-09：切服务器同路径不查询旧记录，也拒绝旧服务器ACK',async()=>{
    const record=await submitted();const oldOrigin=record.originConnectionKey;
    useStore.setState({activeServerConnection:{...useStore.getState().activeServerConnection!,serverId:'other'}});
    await coordinator.reconcileComposerSession(PATH_A);expect(historyMocks.fetch).not.toHaveBeenCalled();
    handleServerMessage({type:'session_user_message',sessionId:`sess-${PATH_A}`,sessionPath:PATH_A,clientMessageId:record.clientMessageId,snapshotVersion:1,message:{id:'entry',text:'old'}},oldOrigin);
    expect(record.acceptance).toBe('unproven');
  });
  it('R04-10：HTTP挂起收到新run_start，迟到idle不能清新busy',async()=>{
    const record=await submitted();let release!:(value:unknown)=>void;
    historyMocks.fetch.mockImplementation(()=>new Promise(resolve=>{release=resolve;}));
    const pending=coordinator.reconcileComposerSession(PATH_A);await vi.advanceTimersByTimeAsync(0);
    coordinator.noteComposerRunEvent({type:'assistant_run_start',sessionId:`sess-${PATH_A}`,sessionPath:PATH_A,streamId:'new-run'});
    release(page(record));await pending;expect(record.acceptance).toBe('accepted');expect(record.runStatus).toBe('running');
    expect(hasInFlightSend(identityOf(PATH_A))).toBe(true);
  });
  it('R04-17：同session单飞，完成不自动轮询；断代迟到响应丢弃',async()=>{
    const record=await submitted();let release!:(value:unknown)=>void;
    historyMocks.fetch.mockImplementation(()=>new Promise(resolve=>{release=resolve;}));
    const a=coordinator.reconcileComposerSession(PATH_A);const b=coordinator.reconcileComposerSession(PATH_A);expect(a).toBe(b);
    await vi.advanceTimersByTimeAsync(0);expect(historyMocks.fetch).toHaveBeenCalledTimes(1);
    noteComposerConnectionClosed();release(page(record));await a;
    expect(record.acceptance).toBe('unproven');await vi.advanceTimersByTimeAsync(30000);expect(historyMocks.fetch).toHaveBeenCalledTimes(1);
  });
  it('R04 ACK缺canonical身份或不同session/version不结算',async()=>{
    const record=await submitted();
    expect(coordinator.noteComposerServerAck(record.clientMessageId)).toBe(false);
    expect(coordinator.noteComposerServerAck(record.clientMessageId,{originConnectionKey:record.originConnectionKey,sessionId:'wrong',sessionPath:PATH_A,snapshotVersion:1,sourceEntryId:'entry'})).toBe(false);
    expect(record.acceptance).toBe('unproven');
  });

  it('R04-05：读取边界忽略abort仍在10秒截止释放单飞并显示失败',async()=>{
    const record=await submitted();historyMocks.fetch.mockImplementation(()=>new Promise(()=>{}));
    let settled=false;void coordinator.reconcileComposerSession(PATH_A).then(()=>{settled=true;});
    await vi.advanceTimersByTimeAsync(10001);
    expect(settled).toBe(true);expect(record.reconciliationStatus).toBe('failed');
    historyMocks.fetch.mockResolvedValue(page(record));await coordinator.reconcileComposerSession(PATH_A);
    expect(record.acceptance).toBe('accepted');
  });
  it('R04-18：未知普通输入阻挡立即插入，不能绕过barrier',async()=>{
    const record=await submitted();noteComposerConnectionClosed();
    expect(tryAcquireSendLease({identity:identityOf(PATH_A),bundle:makeBundle(PATH_A,'插入',{type:'interject'}),targetRun:{streamId:'run',turnId:null}}).ok).toBe(false);
    expect(record.acceptance).toBe('unproven');
  });
  it('R04-02：慢HTTP完成时前台timer已耗尽，idle后仍只续发一次',async()=>{
    const record=await submitted();useStore.getState().endTurnPending(PATH_A);
    enqueue(PATH_A,'next','下一条');requestQueueFlush(PATH_A,makeDeps());
    let release!:(value:unknown)=>void;historyMocks.fetch.mockImplementation(()=>new Promise(resolve=>{release=resolve;}));
    const pending=coordinator.reconcileComposerSession(PATH_A);await vi.advanceTimersByTimeAsync(500);
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    release(page(record));await pending;await vi.advanceTimersByTimeAsync(401);
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(2);
  });
  it('R04-11：真实history-builder水合后canonical ACK合并同一气泡',async()=>{
    const record=await submitted();const {buildItemsFromHistory}=await import('../../utils/history-builder');
    useStore.getState().initSession(PATH_A,buildItemsFromHistory({messages:[{id:'0',entryId:'entry-a',sourceEntryId:'entry-a',clientMessageId:record.clientMessageId,snapshotVersion:1,role:'user',content:'原文  \n'}],blocks:[]}),false);
    handleServerMessage({type:'session_user_message',sessionId:`sess-${PATH_A}`,sessionPath:PATH_A,clientMessageId:record.clientMessageId,snapshotVersion:1,message:{id:'entry-a',text:'server'}});
    const state=useStore.getState();const items=sessionScopedValue(state,state.chatSessions,PATH_A)!.items;
    expect(items).toHaveLength(1);expect(messageData(items[0]).sourceEntryId).toBe('entry-a');
  });
  it('R04-11：ACK未携带字段不擦除已有附件技能和引用',async()=>{
    const record=await submitted();const state=useStore.getState();const old=messageData(sessionScopedValue(state,state.chatSessions,PATH_A)!.items[0]);
    state.appendOptimisticUserMessage(PATH_A,{...old,attachments:[{path:'/synthetic/a',name:'a',isDir:false}],skills:['保留技能'],quotedText:'原引用'});
    dispatchAck(PATH_A,record.clientMessageId);
    const next=useStore.getState();const data=messageData(sessionScopedValue(next,next.chatSessions,PATH_A)!.items[0]);
    expect(data.attachments).toEqual([{path:'/synthetic/a',name:'a',isDir:false}]);expect(data.skills).toEqual(['保留技能']);expect(data.quotedText).toBe('原引用');
  });

});
