// @vitest-environment jsdom

/**
 * composer-send 交易测试（F1 · S01–S15）。
 *
 * 覆盖任务书 P2.6 最低测试集：显式派发结果（blocked/failed_before_submit/
 * delivery_unknown/transport_submitted）、完整快照保留、输入清理时机、
 * 回执关联与断线对账、重试同一逻辑记录。
 * 模块级用例直接驱动 prepare/commit/coordinator；渲染级用例驱动 InputArea。
 * R04 合同迁移：canonical ACK 必须含版本与真实 entry 身份，接收不等于 run 结束；
 * S10 的旧“unknown 后再取租约”与新门禁相违，改为拒绝新发送并保留快照，
 * 另用独立合成会话保留超时断言；S09/S12 的明确未发送重试仍独立验证。
 */

import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

const mocks = vi.hoisted(() => ({
  clearContent: vi.fn(),
  ensureSession: vi.fn(),
  lingxiFetch: vi.fn(),
  upsertOptimisticSessionFirstMessage: vi.fn(),
  editorDoc: { type: 'doc', content: [] } as unknown,
}));

const wsMocks = vi.hoisted(() => ({
  current: null as { send: Mock; readyState: number } | null,
}));

const previewRefreshMocks = vi.hoisted(() => ({
  changeOptions: { retryMissing: true, retryUnchanged: true },
  refreshOpenPreviewDocumentsForResourceChange: vi.fn(async () => undefined),
  markDeskTreeDirtyForResourceChange: vi.fn(),
}));

vi.mock('@tiptap/react', () => ({
  useEditor: () => {
    const chain: Record<string, unknown> = {};
    chain.clearContent = vi.fn(() => chain);
    chain.deleteRange = vi.fn(() => chain);
    chain.insertContent = vi.fn(() => chain);
    chain.focus = vi.fn(() => chain);
    chain.run = vi.fn();
    return {
      commands: {
        focus: vi.fn(),
        clearContent: mocks.clearContent,
        scrollIntoView: vi.fn(),
        setContent: vi.fn(),
        insertContent: vi.fn(),
      },
      chain: () => chain,
      getText: () => '',
      getJSON: () => mocks.editorDoc,
      state: { tr: { setMeta: vi.fn(() => ({})) } },
      view: { dispatch: vi.fn() },
      on: vi.fn(),
      off: vi.fn(),
    };
  },
  EditorContent: () => React.createElement('div', { 'data-testid': 'editor' }),
}));

vi.mock('@tiptap/starter-kit', () => ({
  default: { configure: () => ({}) },
}));

vi.mock('@tiptap/extension-placeholder', () => ({
  default: { configure: () => ({}) },
}));

vi.mock('../../components/input/extensions/skill-badge', () => ({
  SkillBadge: {},
}));

vi.mock('../../hooks/use-stream-buffer', () => ({
  streamBufferManager: {
    handle: vi.fn(),
    beginRun: vi.fn(),
    finishRun: vi.fn(),
  },
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

import { createTestTranslator } from '../helpers/i18n-test-strings';

const testT = createTestTranslator();

vi.mock('../../hooks/use-i18n', () => ({
  useI18n: () => ({ t: testT }),
}));

vi.mock('../../hooks/use-config', () => ({
  fetchConfig: vi.fn(async () => ({})),
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (path: string, opts?: RequestInit) => mocks.lingxiFetch(path, opts),
  lingxiUrl: (path: string) => `http://127.0.0.1:3210${path}`,
}));

vi.mock('../../stores/session-actions', () => ({
  // 未提供权威历史时显式失败，不能用空历史伪造 idle 或漏掉导出。
  fetchSessionHistoryPage: vi.fn(async () => { throw new Error('test_history_unavailable'); }),
  ensureSession: mocks.ensureSession,
  loadSessions: vi.fn(),
  upsertOptimisticSessionFirstMessage: mocks.upsertOptimisticSessionFirstMessage,
}));

vi.mock('../../stores/desk-actions', () => ({
  loadDeskFiles: vi.fn(),
  searchDeskFiles: vi.fn(async () => []),
  toggleJianSidebar: vi.fn(),
}));

vi.mock('../../services/websocket', () => ({
  getWebSocket: vi.fn(() => wsMocks.current),
}));

vi.mock('../../MainContent', () => ({
  attachFilesFromPaths: vi.fn(),
}));

vi.mock('../../components/input/SlashCommandMenu', () => ({
  SlashCommandMenu: () => null,
}));

vi.mock('../../components/input/FileMentionMenu', () => ({
  FileMentionMenu: () => null,
}));

vi.mock('../../components/input/InputStatusBars', () => ({
  InputStatusBars: () => null,
}));

vi.mock('../../components/input/InputContextRow', () => ({
  InputContextRow: () => null,
}));

vi.mock('../../components/input/ComposerToolbar', () => ({
  ComposerToolbar: () => null,
}));

vi.mock('../../components/input/SendButton', () => ({
  SendButton: ({
    disabled,
    onSend,
    isStreaming,
    hasInput,
    onSteer,
    onStop,
  }: {
    disabled?: boolean;
    onSend: () => void;
    isStreaming?: boolean;
    hasInput?: boolean;
    onSteer?: () => void;
    onStop?: () => void;
  }) => React.createElement(
    React.Fragment,
    null,
    React.createElement(
      'button',
      {
        type: 'button',
        'data-testid': 'send',
        disabled: isStreaming ? !hasInput : disabled,
        onClick: isStreaming ? onSteer : onSend,
      },
      'send',
    ),
    isStreaming
      ? React.createElement('button', { type: 'button', 'data-testid': 'stop', onClick: onStop }, 'stop')
      : null,
  ),
}));

vi.mock('../../hooks/use-slash-items', () => ({
  useSkillSlashItems: () => [],
  useServerSlashCommandItems: () => [],
}));

vi.mock('../../utils/paste-upload-feedback', () => ({
  notifyPasteUploadFailure: vi.fn(),
}));

import { InputArea } from '../../components/InputArea';
import { buildFaithfulPasteContent } from '../../utils/editor-serializer';
import { useStore } from '../../stores';
import { sessionScopedValue } from '../../stores/session-slice';
import { handleServerMessage } from '../../services/ws-message-handler';
import {
  dispatchComposerSend,
} from '../../components/input/composer-send';
import {
  findSendRecordByClientMessageId,
  flushQueuedHeadNow,
  getSendRecord,
  noteComposerConnectionClosed,
  noteComposerConnectionOpened,
  resetComposerSendCoordinatorForTests,
  retrySendRecord,
  sendWithLease,
  tryAcquireSendLease,
} from '../../services/composer-send-coordinator';
import type { ComposerSendBundle, QueuedTurnInput } from '../../stores/chat-types';
import { serializeEditor } from '../../utils/editor-serializer';

const PATH = '/session/tx.jsonl';
const SESSION_ID = 'sess_tx';

function makeDeps() {
  return {
    loadVisionAuxiliaryConfig: vi.fn(async () => ({ enabled: false, model: null })),
    t: testT,
  };
}

function makeBundle(text: string, overrides: Partial<ComposerSendBundle> = {}): ComposerSendBundle {
  return {
    type: 'prompt',
    sessionRef: { sessionId: SESSION_ID, sessionPath: PATH, agentId: 'hana' },
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

function seedSession(overrides: Record<string, unknown> = {}) {
  useStore.setState({
    currentSessionPath: PATH,
    currentSessionId: SESSION_ID,
    currentAgentId: 'hana',
    sessions: [{ path: PATH, sessionId: SESSION_ID, agentId: 'hana', agentName: 'Hana' }],
    sessionLocatorsById: { [SESSION_ID]: { path: PATH } },
    connected: true,
    pendingNewSession: false,
    pendingDraftId: null,
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
    inlineErrors: {},
    toasts: [],
    attachedFiles: [],
    attachedFilesBySession: {},
    docContextAttached: false,
    quoteCandidate: null,
    quotedSelections: [],
    quotedSelection: null,
    models: [{ id: 'deepseek-chat', provider: 'deepseek', name: 'DeepSeek Chat', input: ['text'], isCurrent: true }],
    sessionModelsByPath: {},
    previewItems: [],
    previewOpen: false,
    chatSessions: {},
    serverPort: 3210,
    serverToken: null,
    drafts: {},
    draftDocs: {},
    composerRevisionsByKey: {},
    knowledgeReferencesBySession: {},
    ...overrides,
  } as never);
  useStore.getState().clearSession(PATH);
  useStore.getState().initSession(PATH, [], false);
}

function chatItems(path = PATH) {
  const state = useStore.getState();
  return sessionScopedValue(state as never, state.chatSessions, path)?.items || [];
}

/** 取消息项的 data（ChatListItem 是联合类型，先收窄到 message）。 */
function messageData(item: ReturnType<typeof chatItems>[number] | undefined) {
  if (!item || item.type !== 'message') throw new Error('expected message item');
  return item.data;
}

function queueOf(path = PATH): QueuedTurnInput[] {
  const state = useStore.getState();
  return sessionScopedValue(state as never, state.queuedTurnInputsByPath, path) || [];
}

function enqueue(id: string, text: string, overrides: Partial<ComposerSendBundle> = {}) {
  const item: QueuedTurnInput = {
    id,
    sessionPath: PATH,
    text,
    createdAt: Date.now(),
    bundle: makeBundle(text, overrides),
  };
  useStore.getState().enqueueQueuedTurnInput(PATH, item);
  return item;
}

function sentPayloads(): Array<Record<string, unknown>> {
  return wsMocks.current!.send.mock.calls.map(call => JSON.parse(String(call[0])));
}

function dispatchAck(clientMessageId: string) {
  handleServerMessage({
    type: 'session_user_message',
    sessionPath: PATH,
    sessionId: SESSION_ID,
    clientMessageId,
    snapshotVersion: findSendRecordByClientMessageId(clientMessageId)?.snapshotVersion ?? 1,
    message: { id: `srv-${clientMessageId}`, sourceEntryId: `srv-${clientMessageId}`, text: 'acked', timestamp: new Date().toISOString() },
  });
}

// R04：跨子场景续发前必须有同一 canonical 输入的有序权威 run 起止。
function completeAcknowledgedRun(clientMessageId: string): void {
  const runId = `run-${clientMessageId}`;
  const sourceEntryId = `srv-${clientMessageId}`;
  handleServerMessage({ type: 'assistant_run_start', sessionId: SESSION_ID, sessionPath: PATH,
    runId, streamId: runId, seq: 1, turnInputEntryId: sourceEntryId });
  handleServerMessage({ type: 'assistant_run_end', sessionId: SESSION_ID, sessionPath: PATH,
    runId, streamId: runId, seq: 2, turnInputEntryId: sourceEntryId });
  // 本文件 mock 了 stream renderer，只同步投影；coordinator 已独立消费权威事件。
  handleServerMessage({ type: 'status', sessionId: SESSION_ID, sessionPath: PATH, isStreaming: false, streamId: runId });
}

/** 真实计时器下冲刷宏/微任务：替代 fake-timer 的 advanceTimersByTimeAsync(0)。 */
function flushAsync(ms = 0): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

beforeEach(() => {
  vi.clearAllMocks();
  wsMocks.current = { send: vi.fn(), readyState: 1 };
  resetComposerSendCoordinatorForTests();
  seedSession();
  mocks.editorDoc = { type: 'doc', content: [] };
  mocks.ensureSession.mockResolvedValue({ sessionId: SESSION_ID, sessionPath: PATH, agentId: 'hana' });
  mocks.lingxiFetch.mockResolvedValue(new Response(JSON.stringify({
    models: { vision_enabled: true, vision: { id: 'qwen-vl', provider: 'dashscope', input: ['text', 'image'] } },
  }), { status: 200 }));
  window.platform = { readFileBase64: vi.fn(async () => 'SUJBTkVfQkFTRTY0') } as unknown as typeof window.platform;
});

afterEach(() => {
  cleanup();
  resetComposerSendCoordinatorForTests();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  delete (window as { platform?: unknown }).platform;
});

describe('composer-send 显式结果与快照（S01–S15）', () => {
  it('S01：普通发送视频预检失败，编辑器、草稿、附件与引用全部保留', async () => {
    seedSession({
      drafts: { [SESSION_ID]: '还没写完的草稿' },
      attachedFiles: [1, 2, 3, 4].map(i => ({
        fileId: `sf_v${i}`, path: `/tmp/v${i}.mp4`, name: `v${i}.mp4`, mimeType: 'video/mp4', isDirectory: false,
      })),
      attachedFilesBySession: {
        [PATH]: [1, 2, 3, 4].map(i => ({
          fileId: `sf_v${i}`, path: `/tmp/v${i}.mp4`, name: `v${i}.mp4`, mimeType: 'video/mp4', isDirectory: false,
        })),
      },
      quotedSelections: [{ text: '引用的段落', source: 'doc' }],
    });

    render(React.createElement(InputArea));
    fireEvent.click(screen.getByTestId('send'));

    await waitFor(() => {
      expect(useStore.getState().toasts.some(toast => toast.type === 'error')).toBe(true);
    });
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    expect(mocks.clearContent).not.toHaveBeenCalled();
    const state = useStore.getState();
    expect(sessionScopedValue(state as never, state.drafts, PATH)).toBe('还没写完的草稿');
    expect(state.attachedFiles).toHaveLength(4);
    expect(state.quotedSelections).toHaveLength(1);
    expect(chatItems()).toHaveLength(0);
  });

  it('S02：排队项视频/音频读取失败，队列项原位保留、错误可见、无 ws.send', async () => {
    // 视频原生读取失败
    seedSession({
      models: [{ id: 'm-video', provider: 'test', name: 'V', input: ['text', 'video'], isCurrent: true }],
    });
    window.platform = {
      readFileBase64: vi.fn(async () => { throw new Error('disk gone'); }),
    } as unknown as typeof window.platform;
    enqueue('q-video', '看视频', {
      inputFiles: [{ fileId: 'sf_v', path: '/tmp/clip.mp4', name: 'clip.mp4', mimeType: 'video/mp4', isDirectory: false }],
    });

    await flushQueuedHeadNow(PATH, makeDeps());
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    let items = queueOf();
    expect(items).toHaveLength(1);
    expect(items[0].status).toBe('failed');
    expect(items[0].errorCode).toBe('video_read_failed');
    expect(items[0].text).toBe('看视频');

    // 队首失败项不被自动调度跳过，也不无限重试：后续项在位时 flush 不产生发送。
    enqueue('q-audio', '听音频', {
      inputFiles: [{ fileId: 'sf_a', path: '/tmp/voice.wav', name: 'voice.wav', mimeType: 'audio/wav', isDirectory: false }],
    });
    await flushQueuedHeadNow(PATH, makeDeps());
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    expect(queueOf().map(item => item.id)).toEqual(['q-video', 'q-audio']);

    // 用户删除失败的队首项后，音频项才被调度；原生读取同样明确失败。
    useStore.getState().removeQueuedTurnInput(PATH, 'q-video');
    useStore.setState({
      models: [{ id: 'm-audio', provider: 'test', name: 'A', input: ['text', 'audio'], audio: true, audioTransportSupported: true, isCurrent: true }],
    } as never);
    await flushQueuedHeadNow(PATH, makeDeps());
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    items = queueOf();
    expect(items).toHaveLength(1);
    expect(items[0].status).toBe('failed');
    expect(items[0].errorCode).toBe('audio_read_failed');
  });

  it('S03：视频数超限与格式错误都是明确 blocked，不冒充成功', async () => {
    const tooMany = await dispatchComposerSend(makeBundle('看这些', {
      inputFiles: [1, 2, 3, 4].map(i => ({
        fileId: `sf_v${i}`, path: `/tmp/v${i}.mp4`, name: `v${i}.mp4`, mimeType: 'video/mp4', isDirectory: false,
      })),
    }), makeDeps());
    expect(tooMany).toEqual({ kind: 'blocked', code: 'video_count_exceeded', retryable: false });

    seedSession({
      models: [{ id: 'm-video', provider: 'test', name: 'V', input: ['text', 'video'], isCurrent: true }],
    });
    const badFormat = await dispatchComposerSend(makeBundle('看这个', {
      inputFiles: [{ fileId: 'sf_mkv', path: '/tmp/clip.mkv', name: 'clip.mkv', mimeType: 'video/x-matroska', isDirectory: false }],
    }), makeDeps());
    expect(badFormat).toEqual({ kind: 'blocked', code: 'video_format_unsupported', retryable: false });

    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    expect(chatItems()).toHaveLength(0);
  });

  it('S04：ws 不存在 / CONNECTING / CLOSING / CLOSED，明确结果、不清输入、不发消息', async () => {
    for (const [label, socket] of [
      ['null', null],
      ['CONNECTING', { send: vi.fn(), readyState: 0 }],
      ['CLOSING', { send: vi.fn(), readyState: 2 }],
      ['CLOSED', { send: vi.fn(), readyState: 3 }],
    ] as const) {
      wsMocks.current = socket as never;
      const result = await dispatchComposerSend(makeBundle(`hello ${label}`), makeDeps());
      expect(result).toEqual({ kind: 'blocked', code: 'websocket_unavailable', retryable: true });
      expect(chatItems()).toHaveLength(0);
    }

    // 渲染级：ws 不存在时点击发送，编辑器与草稿保留。
    wsMocks.current = null;
    seedSession({ drafts: { [SESSION_ID]: '别丢' } });
    render(React.createElement(InputArea));
    fireEvent.click(screen.getByTestId('send'));
    await flushAsync(20);
    expect(mocks.clearContent).not.toHaveBeenCalled();
    const state = useStore.getState();
    expect(sessionScopedValue(state as never, state.drafts, PATH)).toBe('别丢');
  });

  it('S05：读文件期间会话删除/定位器变化/模型变更，重新验证后旧目标不误发', async () => {
    // 会话被删除。门控读取按调用次序发放 release（每次调用换新 promise）。
    const gate: { release: (value: string) => void } = { release: () => {} };
    window.platform = {
      readFileBase64: vi.fn(() => new Promise<string>((resolve) => { gate.release = resolve; })),
    } as unknown as typeof window.platform;
    const imageBundle = makeBundle('带图', {
      inputFiles: [{ fileId: 'sf_img', path: '/tmp/a.png', name: 'a.png', isDirectory: false }],
    });
    seedSession({
      models: [{ id: 'm-vision', provider: 'test', name: 'V', input: ['text', 'image'], isCurrent: true }],
    });
    const acq = tryAcquireSendLease({ identity: { kind: 'session', sessionId: SESSION_ID, sessionPath: PATH, agentId: 'hana' }, bundle: imageBundle });
    expect(acq.ok).toBe(true);
    if (!acq.ok) return;
    const pending = sendWithLease(acq.leaseId, makeDeps());
    await flushAsync();
    expect(window.platform!.readFileBase64).toHaveBeenCalledTimes(1);
    // 删除证据齐备：投影列表非空但目标会话消失、定位器清空、且不再是当前会话。
    useStore.setState({
      currentSessionPath: null,
      currentSessionId: null,
      sessions: [{ path: '/session/other.jsonl', sessionId: 'sess_other', agentId: 'hana', agentName: 'Hana' }],
      sessionLocatorsById: {},
    } as never);
    gate.release('SU1HX0E=');
    const deleted = await pending;
    expect(deleted).toEqual({ kind: 'blocked', code: 'session_deleted', retryable: false });
    expect(wsMocks.current!.send).not.toHaveBeenCalled();

    // 模型在准备期间变更：载荷按旧模型预检，必须拒绝并按可重试报告。
    seedSession({
      models: [{ id: 'm-vision', provider: 'test', name: 'V', input: ['text', 'image'], isCurrent: true }],
    });
    const acq2 = tryAcquireSendLease({ identity: { kind: 'session', sessionId: SESSION_ID, sessionPath: PATH, agentId: 'hana' }, bundle: imageBundle });
    if (!acq2.ok) throw new Error('lease2');
    const pending2 = sendWithLease(acq2.leaseId, makeDeps());
    await flushAsync();
    expect(window.platform!.readFileBase64).toHaveBeenCalledTimes(2);
    useStore.setState({
      models: [{ id: 'm-other', provider: 'test', name: 'O', input: ['text'], isCurrent: true }],
    } as never);
    gate.release('SU1HX0I=');
    const changed = await pending2;
    expect(changed).toEqual({ kind: 'failed_before_submit', code: 'model_changed', retryable: true });
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
  });

  it('S06：准备期间用户继续输入，原快照独立、新草稿不被清除或覆盖', async () => {
    let release!: (value: string) => void;
    window.platform = {
      readFileBase64: vi.fn(() => new Promise<string>((resolve) => { release = resolve; })),
    } as unknown as typeof window.platform;
    seedSession({
      drafts: { [SESSION_ID]: '原始内容' },
      attachedFiles: [{ fileId: 'sf_img', path: '/tmp/a.png', name: 'a.png', isDirectory: false }],
      attachedFilesBySession: { [PATH]: [{ fileId: 'sf_img', path: '/tmp/a.png', name: 'a.png', isDirectory: false }] },
      models: [{ id: 'm-vision', provider: 'test', name: 'V', input: ['text', 'image'], isCurrent: true }],
    });

    render(React.createElement(InputArea));
    fireEvent.click(screen.getByTestId('send'));
    await waitFor(() => {
      expect(window.platform!.readFileBase64).toHaveBeenCalledTimes(1);
    });

    // 准备期间用户继续输入（每次编辑递增 composerRevision）。
    useStore.getState().setDraft(PATH, '原始内容 + 新输入');

    release('SU1HX0E=');
    await waitFor(() => {
      expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    });
    const payload = sentPayloads()[0];
    expect(String(payload.text)).not.toContain('新输入');
    // 新草稿原样保留，编辑器未被清空、旧快照不回写。
    const state = useStore.getState();
    expect(sessionScopedValue(state as never, state.drafts, PATH)).toBe('原始内容 + 新输入');
    expect(mocks.clearContent).not.toHaveBeenCalled();
  });

  it('S07：文字+文件+skill+agent+quote+notebook 组合，失败与重试保留所有字段', async () => {
    seedSession({
      models: [{ id: 'm-vision', provider: 'test', name: 'V', input: ['text', 'image'], isCurrent: true }],
    });
    const bundle = makeBundle('组合消息', {
      skills: ['skill-a'],
      agentMentions: [{ agentId: 'reviewer', label: 'Reviewer' }] as never,
      inputFiles: [{
        fileId: 'sf_img', path: '/tmp/a.png', name: 'a.png', isDirectory: false,
        base64Data: 'SU1HX0lOTElORQ==', mimeType: 'image/png',
      } as never],
      knowledgeRefs: { notebookIds: ['nb1'], notebookNames: { nb1: '笔记一' }, mode: 'auto' },
      quotes: [{ text: '引用片段', source: 'chat' }] as never,
    });
    const acq = tryAcquireSendLease({ identity: { kind: 'session', sessionId: SESSION_ID, sessionPath: PATH, agentId: 'hana' }, bundle });
    if (!acq.ok) throw new Error('lease');
    wsMocks.current!.send.mockImplementationOnce(() => { throw new Error('boom'); });
    const failed = await sendWithLease(acq.leaseId, makeDeps());
    expect(failed.kind).toBe('failed_before_submit');

    // 记录内快照完整保留所有字段。
    const record = getSendRecord(acq.leaseId)!;
    expect(record.bundle.text).toBe('组合消息');
    expect(record.bundle.skills).toEqual(['skill-a']);
    expect(record.bundle.agentMentions).toHaveLength(1);
    expect(record.bundle.inputFiles).toHaveLength(1);
    expect(record.bundle.knowledgeRefs?.notebookIds).toEqual(['nb1']);
    expect(record.bundle.quotes).toHaveLength(1);

    // 重试：同一条逻辑记录，全部字段进入载荷。
    const retried = await retrySendRecord(acq.leaseId, makeDeps());
    expect(retried?.kind).toBe('transport_submitted');
    const payload = sentPayloads().at(-1)!;
    expect(payload.text).toContain('组合消息');
    expect(payload.text).toContain('引用片段');
    expect(payload.skills).toEqual(['skill-a']);
    expect(payload.agentReviewRequests).toEqual([{ agentId: 'reviewer', label: 'Reviewer' }]);
    expect(payload.knowledgeRefs).toEqual({ notebookIds: ['nb1'], mode: 'auto' });
    expect(payload.images).toEqual([{ type: 'image', data: 'SU1HX0lOTElORQ==', mimeType: 'image/png' }]);
    expect(chatItems()).toHaveLength(1);
  });

  it('S08：图片按既有规则退化为文件引用，提示保留、文件身份不丢', async () => {
    // 文本模型 + 无辅助视觉：图片降级为文件引用但仍发送。
    const result = await dispatchComposerSend(makeBundle('看图', {
      inputFiles: [{ fileId: 'sf_img', path: '/tmp/a.png', name: 'a.png', isDirectory: false }],
    }), makeDeps());
    expect(result.kind).toBe('transport_submitted');
    const payload = sentPayloads()[0];
    expect(payload.images).toBeUndefined();
    expect(payload.displayMessage).toMatchObject({
      attachments: [expect.objectContaining({ fileId: 'sf_img', path: '/tmp/a.png', name: 'a.png' })],
    });
    expect(window.platform!.readFileBase64).not.toHaveBeenCalled();
    expect(useStore.getState().toasts.length).toBeGreaterThan(0);
  });

  it('S09：ws.send 同步抛错，正确标失败、完整快照可按同一记录重试', async () => {
    wsMocks.current!.send.mockImplementationOnce(() => { throw new Error('socket exploded'); });
    const acq = tryAcquireSendLease({ identity: { kind: 'session', sessionId: SESSION_ID, sessionPath: PATH, agentId: 'hana' }, bundle: makeBundle('会失败') });
    if (!acq.ok) throw new Error('lease');
    const failed = await sendWithLease(acq.leaseId, makeDeps());
    expect(failed).toEqual({ kind: 'failed_before_submit', code: 'ws_send_threw', retryable: true });

    // 乐观消息被标记失败（同一条记录），快照保留。
    const items = chatItems();
    expect(items).toHaveLength(1);
    expect(messageData(items[0]).sendStatus).toBe('failed');
    expect(messageData(items[0]).sendError).toContain('socket exploded');

    const retried = await retrySendRecord(acq.leaseId, makeDeps());
    expect(retried?.kind).toBe('transport_submitted');
    expect(chatItems()).toHaveLength(1);
    const payloads = sentPayloads();
    expect(payloads).toHaveLength(2);
    expect(payloads[1].clientMessageId).toBe(payloads[0].clientMessageId);
  });

  it('S10：提交后断线无回执与回执超时，都标 delivery_unknown 且不自动重发', async () => {
    // 回执看门狗依赖 15s 计时器：本用例独占 fake timers（afterEach 恢复）。
    vi.useFakeTimers({ shouldAdvanceTime: false });
    // 断线路径
    const acq = tryAcquireSendLease({ identity: { kind: 'session', sessionId: SESSION_ID, sessionPath: PATH, agentId: 'hana' }, bundle: makeBundle('断线') });
    if (!acq.ok) throw new Error('lease');
    const sent = await sendWithLease(acq.leaseId, makeDeps());
    expect(sent.kind).toBe('transport_submitted');
    noteComposerConnectionClosed();
    const record = getSendRecord(acq.leaseId)!;
    expect(record.phase).toBe('delivery_unknown');
    expect(record.code).toBe('connection_closed');
    noteComposerConnectionOpened();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    // 未知投递禁止盲目重试。
    expect(await retrySendRecord(acq.leaseId, makeDeps())).toBeNull();

    // R04：unknown 是仍持有的输入，不可通过新 lease/普通消息绕过。
    const blockedNew = tryAcquireSendLease({ identity: { kind: 'session', sessionId: SESSION_ID, sessionPath: PATH, agentId: 'hana' }, bundle: makeBundle('不能越过未知') });
    expect(blockedNew.ok).toBe(false);
    expect(record.bundle.text).toBe('断线');
    expect(chatItems()).toHaveLength(1);
    expect(messageData(chatItems()[0]).text).toBe('断线');

    // 保留原 ACK 超时反例，但在独立合成会话验证，不能隐式解锁上一个 unknown。
    const timeoutPath = '/session/timeout.jsonl';
    const timeoutSessionId = 'sess_timeout';
    const timeoutBundle = makeBundle('超时', { sessionRef: { sessionId: timeoutSessionId, sessionPath: timeoutPath, agentId: 'hana' } });
    useStore.setState(state => ({ sessions: [...state.sessions, { path: timeoutPath, sessionId: timeoutSessionId, agentId: 'hana' } as never],
      sessionLocatorsById: { ...state.sessionLocatorsById, [timeoutSessionId]: { path: timeoutPath } } }));
    const acq2 = tryAcquireSendLease({ identity: { kind: 'session', sessionId: timeoutSessionId, sessionPath: timeoutPath, agentId: 'hana' }, bundle: timeoutBundle });
    if (!acq2.ok) throw new Error('independent timeout session lease');
    const sent2 = await sendWithLease(acq2.leaseId, makeDeps());
    expect(sent2.kind).toBe('transport_submitted');
    // 同步观察看门狗边界，后续异步对账诊断不能覆盖本条超时来源断言。
    vi.advanceTimersByTime(15_000);
    const record2 = getSendRecord(acq2.leaseId)!;
    expect(record2.phase).toBe('delivery_unknown');
    expect(record2.code).toBe('ack_timeout');
    expect(record2.bundle.text).toBe('超时');
    expect(await retrySendRecord(acq2.leaseId, makeDeps())).toBeNull();
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(2);
    expect(record.phase).toBe('delivery_unknown');
    expect(tryAcquireSendLease({ identity: { kind: 'session', sessionId: timeoutSessionId, sessionPath: timeoutPath, agentId: 'hana' }, bundle: timeoutBundle }).ok).toBe(false);
  });

  it('S11：重连后经恢复/历史找到原 clientMessageId，合并不重复追加', async () => {
    const acq = tryAcquireSendLease({ identity: { kind: 'session', sessionId: SESSION_ID, sessionPath: PATH, agentId: 'hana' }, bundle: makeBundle('可能已送达') });
    if (!acq.ok) throw new Error('lease');
    await sendWithLease(acq.leaseId, makeDeps());
    const clientMessageId = String(sentPayloads()[0].clientMessageId);
    noteComposerConnectionClosed();
    expect(getSendRecord(acq.leaseId)?.phase).toBe('delivery_unknown');
    expect(messageData(chatItems()[0]).sendStatus).toBe('failed');

    // 重连后恢复通道回放同一 clientMessageId 的用户消息：绑定既有消息与记录。
    noteComposerConnectionOpened();
    dispatchAck(clientMessageId);
    expect(getSendRecord(acq.leaseId)?.phase).toBe('accepted');
    const items = chatItems();
    expect(items).toHaveLength(1);
    expect(messageData(items[0]).sendStatus).toBeUndefined();
    expect(messageData(items[0]).text).toBe('可能已送达');
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
  });

  it('S12：明确失败后重试，仍是一条逻辑记录、内容与上下文一致', async () => {
    // 队首因 ws.send 抛错失败 → 显式重试（同 queueItemId 复用 clientMessageId）。
    wsMocks.current!.send.mockImplementationOnce(() => { throw new Error('boom'); });
    const queuedBody = '\n排队内容  \n';
    enqueue('q-1', queuedBody, { skills: ['skill-x'] });
    await flushQueuedHeadNow(PATH, makeDeps());
    let items = queueOf();
    expect(items).toHaveLength(1);
    expect(items[0].status).toBe('failed');

    const firstClientMessageId = String(sentPayloads()[0].clientMessageId);
    useStore.getState().setQueuedTurnInputStatus(PATH, 'q-1', 'ready');
    await flushQueuedHeadNow(PATH, makeDeps());
    expect(queueOf()).toHaveLength(0);
    const payloads = sentPayloads();
    expect(payloads).toHaveLength(2);
    expect(payloads[1].clientMessageId).toBe(firstClientMessageId);
    expect(payloads[1].text).toBe(queuedBody);
    expect(payloads[1].skills).toEqual(['skill-x']);
    // 重试更新同一条乐观消息，不追加第二条用户消息。
    expect(chatItems()).toHaveLength(1);

    // 回执释放传输租约、回合门禁清空后，下一项才允许进入准备（F2 串行）。
    dispatchAck(firstClientMessageId);
    // R04：原视频失败断言保留，先用真正 run 终态结束前一成功重试。
    completeAcknowledgedRun(firstClientMessageId);

    // 不可重试的 blocked 项（视频超限）拒绝重试。
    enqueue('q-2', '四段视频', {
      inputFiles: [1, 2, 3, 4].map(i => ({
        fileId: `sf_v${i}`, path: `/tmp/v${i}.mp4`, name: `v${i}.mp4`, mimeType: 'video/mp4', isDirectory: false,
      })),
    });
    await flushQueuedHeadNow(PATH, makeDeps());
    items = queueOf();
    expect(items[0].status).toBe('blocked');
    expect(items[0].retryable).toBe(false);
  });

  it('S13：带首行空格与末尾换行的输入，快照逐字符保留', async () => {
    // 模块级：快照与载荷都不得 trim。
    const rawText = '  首行有空格\n';
    const acq = tryAcquireSendLease({ identity: { kind: 'session', sessionId: SESSION_ID, sessionPath: PATH, agentId: 'hana' }, bundle: makeBundle(rawText) });
    if (!acq.ok) throw new Error('lease');
    wsMocks.current = null;
    const blocked = await sendWithLease(acq.leaseId, makeDeps());
    expect(blocked.kind).toBe('blocked');
    expect(getSendRecord(acq.leaseId)?.bundle.text).toBe(rawText);
    wsMocks.current = { send: vi.fn(), readyState: 1 };
    const retried = await retrySendRecord(acq.leaseId, makeDeps());
    expect(retried?.kind).toBe('transport_submitted');
    expect(sentPayloads()[0].text).toBe(rawText);

    // 模块级发送仍持有传输租约（awaiting_ack）：回执释放、回合门禁清空后再进渲染级用例。
    dispatchAck(String(sentPayloads()[0].clientMessageId));
    // R04：渲染级保真子例之前完成前一条真实回合，不能只清 pending。
    completeAcknowledgedRun(String(sentPayloads()[0].clientMessageId));

    // 渲染级：编辑器序列化结果不经过发送链的二次改写，逐字符进入载荷。
    cleanup();
    wsMocks.current = { send: vi.fn(), readyState: 1 };
    seedSession({ drafts: { [SESSION_ID]: ' x' } });
    mocks.editorDoc = {
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'text', text: '  保留我  ' }] }],
    };
    render(React.createElement(InputArea));
    fireEvent.click(screen.getByTestId('send'));
    await waitFor(() => {
      expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    });
    const raw = serializeEditor(mocks.editorDoc as never).text;
    expect(sentPayloads()[0].text).toBe(raw);
    // F10/P7.3 已移除 serializeEditor 的边缘 trim（原 L184）：边缘空白随快照
    // 逐字符保留（P01/P02 端到端验收），发送链不得在此基础上再 trim。
    expect(raw).toBe('  保留我  ');
    expect(sentPayloads()[0].text).toBe('  保留我  ');
  });

  it('S14：首页 ensureSession 期间重复点击，只创建/提交一次', async () => {
    let releaseCreate!: () => void;
    const createGate = new Promise<void>((resolve) => { releaseCreate = resolve; });
    mocks.ensureSession.mockImplementation(async () => {
      await createGate;
      return { sessionId: 'sess_a', sessionPath: '/session/a.jsonl', agentId: 'hana' };
    });
    seedSession({
      currentSessionPath: null,
      currentSessionId: null,
      pendingNewSession: true,
      pendingDraftId: 'draft-a',
      sessions: [],
      sessionLocatorsById: {},
      attachedFiles: [{ fileId: 'sf_a', path: '/tmp/a.png', name: 'a.png', isDirectory: false }],
      attachedFilesBySession: {},
    });

    render(React.createElement(InputArea));
    const send = screen.getByTestId('send');
    fireEvent.click(send);
    fireEvent.click(send);
    await waitFor(() => {
      expect(mocks.ensureSession).toHaveBeenCalledTimes(1);
    });
    releaseCreate();
    await waitFor(() => {
      expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    });
    expect(sentPayloads()[0].sessionId).toBe('sess_a');
    expect(mocks.ensureSession).toHaveBeenCalledTimes(1);
  });

  it('S15：入队后改变界面选中的笔记本，已排队消息仍使用入队快照范围', async () => {
    enqueue('q-nb', '查笔记', {
      knowledgeRefs: { notebookIds: ['nb1'], notebookNames: { nb1: '第一本' }, mode: 'auto' },
    });
    // 用户在界面改选另一本笔记（对已排队消息不应产生影响）。
    useStore.getState().toggleKnowledgeNotebook(PATH, 'nb2', '第二本');

    await flushQueuedHeadNow(PATH, makeDeps());
    expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    const payload = sentPayloads()[0];
    expect(payload.knowledgeRefs).toEqual({ notebookIds: ['nb1'], mode: 'auto' });
  });

  // ── F10/P7 粘贴到发送文本保真（P07/P08/P10）──

  it('P07：全空白正文（无其他内容维度）不发，且不做破坏性清空', async () => {
    mocks.editorDoc = { type: 'doc', content: [buildFaithfulPasteContent('   \n\t  ')] };
    render(React.createElement(InputArea));
    fireEvent.click(screen.getByTestId('send'));
    await flushAsync();
    // 空输入静默拒绝：不发送、不清空（编辑器内容原样保留）
    expect(wsMocks.current!.send).not.toHaveBeenCalled();
    expect(mocks.clearContent).not.toHaveBeenCalled();
  });

  it('P08：全空白正文但有附件 → 按已有内容维度发送，wire 保真、附件身份保留', async () => {
    mocks.editorDoc = { type: 'doc', content: [buildFaithfulPasteContent('   ')] };
    seedSession({
      attachedFiles: [{ fileId: 'sf_doc', path: '/tmp/hana/session-files/doc.txt', name: 'doc.txt', isDirectory: false }],
      attachedFilesBySession: {
        [PATH]: [{ fileId: 'sf_doc', path: '/tmp/hana/session-files/doc.txt', name: 'doc.txt', isDirectory: false }],
      },
    });
    render(React.createElement(InputArea));
    fireEvent.click(screen.getByTestId('send'));
    await waitFor(() => {
      expect(wsMocks.current!.send).toHaveBeenCalledTimes(1);
    });
    const payload = sentPayloads()[0];
    const display = payload.displayMessage as { text: string; attachments: Array<Record<string, unknown>> };
    // 用户正文原样（空白不丢）；文件块按协议追加（独立断言，不借追加掩盖正文）
    expect(payload.text).toBe('   \n\n[附件] doc.txt');
    expect(display.text).toBe('   ');
    expect(display.attachments[0]).toMatchObject({
      fileId: 'sf_doc',
      path: '/tmp/hana/session-files/doc.txt',
      name: 'doc.txt',
    });
  });

  it('P10：排队 → 编辑 → 失败 → 重试 → wire，每次都不 trim 正文', async () => {
    wsMocks.current!.send.mockImplementationOnce(() => { throw new Error('boom'); });
    const padded = '  排队原文  \n\t尾行\n';
    enqueue('q-f10', padded);
    await flushQueuedHeadNow(PATH, makeDeps());
    expect(queueOf()[0].status).toBe('failed');
    expect(queueOf()[0].text).toBe(padded);

    // 编辑保存 = 新快照：正文逐字符保留（含首尾空白与空行）
    const edited = '  改后保留首尾空格  ';
    useStore.getState().updateQueuedTurnInputText(PATH, 'q-f10', edited);
    const item = queueOf()[0];
    expect(item.text).toBe(edited);
    expect(item.bundle.text).toBe(edited);

    useStore.getState().setQueuedTurnInputStatus(PATH, 'q-f10', 'ready');
    await flushQueuedHeadNow(PATH, makeDeps());
    expect(queueOf()).toHaveLength(0);
    const lastPayload = sentPayloads()[sentPayloads().length - 1];
    expect(lastPayload.text).toBe(edited);
    expect((lastPayload.displayMessage as { text: string }).text).toBe(edited);
  });
});

describe('R03 InputArea 编辑点击接线', () => {
  it('R03-03：点击编辑同步取消慢附件，保存后仅提交新正文', async () => {
    let release!: (value: string) => void;
    const gate = new Promise<string>(resolve => { release = resolve; });
    window.platform = { readFileBase64: vi.fn(() => gate) } as unknown as typeof window.platform;
    seedSession({ models: [{ id: 'vision', provider: 'test', name: 'Vision', input: ['text', 'image'], isCurrent: true }] });
    enqueue('ui-edit', '旧正文', { inputFiles: [{ fileId: 'img', path: '/tmp/test.png', name: 'test.png', isDirectory: false }] });
    const view = render(React.createElement(InputArea));
    const pending = flushQueuedHeadNow(PATH, makeDeps());
    await waitFor(() => expect(window.platform.readFileBase64).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: /编辑|Edit/i }));
    expect(queueOf()[0].editing).toBe(true);
    fireEvent.change(screen.getByRole('textbox'), { target: { value: '  新正文\n' } });
    await act(async () => { release('SU1H'); await pending; });
    expect(sentPayloads()).toHaveLength(0);
    fireEvent.click(screen.getByRole('button', { name: /确认|Confirm/i }));
    await waitFor(() => expect(sentPayloads()).toHaveLength(1));
    expect(sentPayloads()[0].text).toBe('  新正文\n');
    view.unmount();
  });

  it('R03-05：点击取消保留原 blocked 状态与正文', () => {
    enqueue('ui-cancel', '原正文');
    useStore.getState().setQueuedTurnInputStatus(PATH, 'ui-cancel', 'blocked', 'vision_disabled', false);
    render(React.createElement(InputArea));
    fireEvent.click(screen.getByRole('button', { name: /编辑|Edit/i }));
    fireEvent.change(screen.getByRole('textbox'), { target: { value: '未保存修改' } });
    fireEvent.click(screen.getByRole('button', { name: /取消|Cancel/i }));
    expect(queueOf()[0]).toMatchObject({ text: '原正文', status: 'blocked', errorCode: 'vision_disabled', retryable: false, editing: false });
    expect(sentPayloads()).toHaveLength(0);
  });
});

it('R03-09：真实 InputArea StrictMode 卸载撤销 timer，重挂只发一次', async () => {
  vi.useFakeTimers();
  enqueue('strict-ui', '只发一次');
  const first = render(React.createElement(React.StrictMode, null, React.createElement(InputArea)));
  first.unmount();
  await act(async () => { await vi.advanceTimersByTimeAsync(400); });
  expect(sentPayloads()).toHaveLength(0);
  render(React.createElement(React.StrictMode, null, React.createElement(InputArea)));
  await act(async () => { await vi.advanceTimersByTimeAsync(400); });
  expect(sentPayloads()).toHaveLength(1);
});

it('R03-12：InputArea A 慢附件时切 B 可发送，A 完成不再清 B 编辑器', async () => {
  const otherPath = '/session/b-ui.jsonl';
  let release!: (value: string) => void;
  const gate = new Promise<string>(resolve => { release = resolve; });
  window.platform = { readFileBase64: vi.fn(() => gate) } as unknown as typeof window.platform;
  seedSession({
    sessions: [{ path: PATH, sessionId: SESSION_ID, agentId: 'hana' }, { path: otherPath, sessionId: 'b-ui', agentId: 'hana' }],
    sessionLocatorsById: { [SESSION_ID]: { path: PATH }, 'b-ui': { path: otherPath } },
    models: [{ id: 'vision', provider: 'test', input: ['text', 'image'], isCurrent: true }],
    attachedFiles: [{ fileId: 'a-img', path: '/tmp/a.png', name: 'a.png', isDirectory: false }],
  });
  mocks.editorDoc = buildFaithfulPasteContent('A 正文');
  render(React.createElement(InputArea));
  fireEvent.click(screen.getByTestId('send'));
  await waitFor(() => expect(window.platform.readFileBase64).toHaveBeenCalled());
  mocks.editorDoc = buildFaithfulPasteContent('B 正文');
  act(() => { useStore.setState({ currentSessionPath: otherPath, currentSessionId: 'b-ui', attachedFiles: [] }); });
  fireEvent.click(screen.getByTestId('send'));
  await waitFor(() => expect(sentPayloads()).toHaveLength(1));
  expect(sentPayloads()[0].sessionPath).toBe(otherPath);
  const clearedByB = mocks.clearContent.mock.calls.length;
  await act(async () => { release('SU1H'); });
  await waitFor(() => expect(sentPayloads()).toHaveLength(2));
  expect(sentPayloads()[1].sessionPath).toBe(PATH);
  expect(mocks.clearContent).toHaveBeenCalledTimes(clearedByB);
});
