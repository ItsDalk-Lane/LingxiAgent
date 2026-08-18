/**
 * chat-turn-lifecycle — Turn 生命周期权威化回归测试
 *
 * 从 handleServerMessage 入口驱动真实 StreamBufferManager + store，
 * 覆盖三个实机 P0：
 *   1) status true/false 抖动不得在 turn_end 前形成 Process Fold
 *   2) turn_end 之前不得出现 missing_final_answer
 *   3) turn_end 的 finalization 必须 exactly once
 *
 * 核心不变量：Session Busy（status）与 Assistant Turn Lifecycle（turn_start/turn_end）
 * 是两个正交状态；status 的任何组合都不得改变内容生命周期。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../services/stream-resume', () => ({
  replayStreamResume: vi.fn(),
  isStreamResumeRebuilding: () => null,
  isStreamScopedMessage: () => false,
  updateSessionStreamMeta: vi.fn(),
}));

vi.mock('../../services/stream-key-dispatcher', () => ({
  dispatchStreamKey: vi.fn(),
}));

vi.mock('../../stores/session-actions', () => ({
  loadSessions: vi.fn(),
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
  PREVIEW_DOCUMENT_CHANGE_REFRESH_OPTIONS: {},
  refreshOpenPreviewDocumentsForResourceChange: vi.fn(async () => undefined),
  markDeskTreeDirtyForResourceChange: vi.fn(),
}));

import { handleServerMessage, configureWsMessageHandler } from '../../services/ws-message-handler';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import { useStore } from '../../stores';
import type { AssistantTurnStatus, ChatListItem } from '../../stores/chat-types';
import { readLiveAssistantMessage } from '../../stores/live-turn-store';
import { buildTranscriptRenderItems } from '../../components/chat/process-fold';
import { resetSessionRefreshSchedulerForTest } from '../../services/session-refresh-scheduler';

const PATH = '/test/turn-lifecycle.jsonl';
const SID = 'sess_turn_lifecycle';
const STREAM_ID = 'stream-turn-1';

let seq = 0;

function send(msg: Record<string, unknown>): void {
  seq += 1;
  handleServerMessage({
    sessionPath: PATH,
    sessionId: SID,
    streamId: STREAM_ID,
    seq,
    ...msg,
  });
}

function sendStatus(isStreaming: boolean): void {
  handleServerMessage({
    type: 'status',
    sessionPath: PATH,
    sessionId: SID,
    isStreaming,
    streamId: STREAM_ID,
  });
}

function getItems(): ChatListItem[] {
  const state: any = useStore.getState();
  return state.chatSessions[SID]?.items ?? state.chatSessions[PATH]?.items ?? [];
}

function assistantMessages(): Array<Extract<ChatListItem, { type: 'message' }>> {
  return getItems().filter((item): item is Extract<ChatListItem, { type: 'message' }> => (
    item.type === 'message' && item.data.role === 'assistant'
  ));
}

function latestAssistant(): Extract<ChatListItem, { type: 'message' }> | null {
  const all = assistantMessages();
  return all.length > 0 ? all[all.length - 1] : null;
}

function liveSnapshot() {
  const latest = latestAssistant();
  if (!latest) return null;
  return readLiveAssistantMessage(PATH, latest.data.id);
}

function liveTurnStatus(): AssistantTurnStatus | null {
  return liveSnapshot()?.turnProjection?.status ?? null;
}

function isSessionBusy(): boolean {
  return useStore.getState().streamingSessions.includes(PATH);
}

/** 用 ChatTranscript 同一份投影逻辑判断当前会不会形成 Process Fold。 */
function renderNow() {
  return buildTranscriptRenderItems(getItems(), {
    isStreaming: isSessionBusy(),
    liveTurnStatus: liveTurnStatus(),
  });
}

function foldCount(): number {
  return renderNow().filter((item) => item.type === 'process_fold').length;
}

/** 已落 store 的 assistant 块里的 turn_status 数量（live 期只看 live 投影）。 */
function committedTurnStatusCount(): number {
  return assistantMessages().reduce((count, item) => (
    count + (item.data.blocks || []).filter((block) => block.type === 'turn_status').length
  ), 0);
}

function liveBlocks() {
  return [...(liveSnapshot()?.blocks || [])];
}

function liveToolCount(): number {
  return liveBlocks().reduce((count, block) => (
    block.type === 'tool_group' ? count + block.tools.length : count
  ), 0);
}

beforeEach(() => {
  seq = 0;
  configureWsMessageHandler({ requestContextUsage: () => {} });
  streamBufferManager.clearAll();
  useStore.setState({
    currentSessionId: SID,
    currentSessionPath: PATH,
    sessionLocatorsById: { [SID]: { path: PATH } },
    pendingNewSession: false,
    sessions: [{
      path: PATH,
      sessionId: SID,
      title: null,
      firstMessage: '',
      modified: '',
      messageCount: 0,
      agentId: 'agent-1',
    }],
    agents: [{ id: 'agent-1', yuan: 'lingxi' }],
    streamingSessions: [],
    activeSessionStreams: {},
    unreadOutputSessionPaths: [],
    inlineErrors: {},
  } as never);
  useStore.getState().clearSession(PATH);
  useStore.getState().initSession(PATH, [
    { type: 'message', data: { id: 'u1', role: 'user', text: 'hi' } },
  ], false);
});

afterEach(() => {
  resetSessionRefreshSchedulerForTest();
});

describe('Turn 生命周期与 Session Busy 解耦', () => {
  it('重复的 status true 不得 finalize 当前 Turn', () => {
    send({ type: 'turn_start' });
    sendStatus(true);
    send({ type: 'thinking_start' });
    send({ type: 'thinking_delta', delta: '先想想' });
    send({ type: 'thinking_end' });
    send({ type: 'tool_start', id: 'call-1', name: 'read', args: { path: '/a.md' } });
    send({ type: 'tool_end', id: 'call-1', name: 'read', success: true });

    // 服务器抖出第二个 status true（同一条流）
    sendStatus(true);

    // 断言：Turn 仍在 streaming，没有 fold，没有 missing_final_answer，工具卡仍在
    expect(liveTurnStatus()).toBe('streaming');
    expect(foldCount()).toBe(0);
    expect(committedTurnStatusCount()).toBe(0);
    expect(liveToolCount()).toBe(1);

    send({ type: 'tool_start', id: 'call-2', name: 'write', args: { path: '/b.md' } });
    send({ type: 'tool_end', id: 'call-2', name: 'write', success: true });
    send({ type: 'text_delta', delta: '最终回答。' });
    send({ type: 'turn_end', assistantEntryId: 'entry-a1', turnInputEntryId: 'entry-u1' });
    sendStatus(false);

    expect(assistantMessages()).toHaveLength(1);
    expect(foldCount()).toBe(1);
    expect(committedTurnStatusCount()).toBe(0);
    const blocks = latestAssistant()?.data.blocks || [];
    expect(blocks.some((block) => block.type === 'text' && block.source === '最终回答。')).toBe(true);
    // 两个工具都必须在（顺序完成的工具各占一个 tool_group，这是既有行为）
    const toolTotal = blocks.reduce((count, block) => (
      block.type === 'tool_group' ? count + block.tools.length : count
    ), 0);
    expect(toolTotal).toBe(2);
  });

  it('status false 出现在真正的 turn_end 之前时，不得结束当前 Turn', () => {
    send({ type: 'turn_start' });
    sendStatus(true);
    send({ type: 'thinking_start' });
    send({ type: 'thinking_delta', delta: '思考中' });
    send({ type: 'thinking_end' });
    send({ type: 'tool_start', id: 'call-1', name: 'read', args: {} });
    send({ type: 'tool_end', id: 'call-1', name: 'read', success: true });

    // 提前到达的 status false：只代表 Session Busy 抖动，不代表 Turn 结束
    sendStatus(false);

    expect(liveTurnStatus()).toBe('streaming');
    expect(foldCount()).toBe(0);
    expect(committedTurnStatusCount()).toBe(0);
    // live blocks 没有被 commit/reset：工具卡与思考仍然可见
    expect(liveToolCount()).toBe(1);
    expect(liveBlocks().some((block) => block.type === 'thinking')).toBe(true);
    // store 里的消息不得被提前写入终态投影
    expect(latestAssistant()?.data.turnProjection).toBeUndefined();

    send({ type: 'tool_start', id: 'call-2', name: 'exec', args: {} });
    send({ type: 'tool_end', id: 'call-2', name: 'exec', success: true });
    send({ type: 'mood_start' });
    send({ type: 'mood_text', delta: 'Vibe: 稳' });
    send({ type: 'mood_end' });
    send({ type: 'text_delta', delta: '做完了。' });
    send({ type: 'turn_end', assistantEntryId: 'entry-a1', turnInputEntryId: 'entry-u1' });

    expect(assistantMessages()).toHaveLength(1);
    expect(foldCount()).toBe(1);
    expect(committedTurnStatusCount()).toBe(0);
    const blocks = latestAssistant()?.data.blocks || [];
    expect(blocks.some((block) => block.type === 'mood')).toBe(true);
    expect(blocks.some((block) => block.type === 'text' && block.source === '做完了。')).toBe(true);
  });

  it('status 高频抖动全程：turn_end 前 fold=0 / missing=0，turn_end 后各出现一次', () => {
    send({ type: 'turn_start' });
    sendStatus(true);
    sendStatus(false);
    sendStatus(true);
    sendStatus(false);
    sendStatus(true);

    send({ type: 'thinking_start' });
    send({ type: 'thinking_delta', delta: '想' });
    send({ type: 'thinking_end' });

    sendStatus(false);
    expect(foldCount()).toBe(0);
    expect(committedTurnStatusCount()).toBe(0);

    send({ type: 'tool_start', id: 'call-1', name: 'read', args: {} });
    send({ type: 'tool_end', id: 'call-1', name: 'read', success: true });

    sendStatus(true);
    expect(foldCount()).toBe(0);
    expect(committedTurnStatusCount()).toBe(0);

    send({ type: 'text_delta', delta: '答案。' });

    sendStatus(false);
    expect(foldCount()).toBe(0);
    expect(committedTurnStatusCount()).toBe(0);
    expect(liveTurnStatus()).toBe('streaming');

    send({ type: 'turn_end', assistantEntryId: 'entry-a1', turnInputEntryId: 'entry-u1' });

    expect(foldCount()).toBe(1);
    expect(committedTurnStatusCount()).toBe(0);
    expect(assistantMessages()).toHaveLength(1);
    const blocks = latestAssistant()?.data.blocks || [];
    expect(blocks.filter((block) => block.type === 'text')).toHaveLength(1);
  });

  it('重复 turn_end 幂等：commit 只发生一次，不产生第二个 AssistantMessage', () => {
    send({ type: 'turn_start' });
    sendStatus(true);
    send({ type: 'thinking_start' });
    send({ type: 'thinking_delta', delta: '想' });
    send({ type: 'thinking_end' });
    send({ type: 'turn_end', assistantEntryId: 'entry-a1', turnInputEntryId: 'entry-u1' });

    expect(assistantMessages()).toHaveLength(1);
    expect(foldCount()).toBe(1);
    // 真正无 final answer 的终结 Turn（无工具调用，循环已停）：missing 恰好出现一次
    expect(committedTurnStatusCount()).toBe(1);

    // 服务器重发同一个 turn_end（重连 replay 场景）
    send({ type: 'turn_end', assistantEntryId: 'entry-a1', turnInputEntryId: 'entry-u1' });

    expect(assistantMessages()).toHaveLength(1);
    expect(foldCount()).toBe(1);
    expect(committedTurnStatusCount()).toBe(1);
  });

  it('turn_start 幂等：同一 streamId 重复 turn_start 不得结束当前 Turn', () => {
    send({ type: 'turn_start' });
    sendStatus(true);
    send({ type: 'tool_start', id: 'call-1', name: 'read', args: {} });
    send({ type: 'tool_end', id: 'call-1', name: 'read', success: true });

    send({ type: 'turn_start' });

    expect(liveTurnStatus()).toBe('streaming');
    expect(foldCount()).toBe(0);
    expect(committedTurnStatusCount()).toBe(0);
    expect(liveToolCount()).toBe(1);

    send({ type: 'text_delta', delta: '好。' });
    send({ type: 'turn_end', assistantEntryId: 'entry-a1', turnInputEntryId: 'entry-u1' });
    expect(assistantMessages()).toHaveLength(1);
    expect(foldCount()).toBe(1);
    expect(committedTurnStatusCount()).toBe(0);
  });
});
