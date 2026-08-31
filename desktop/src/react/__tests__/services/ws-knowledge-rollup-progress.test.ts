import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../stores/session-actions', () => ({ loadSessions: vi.fn() }));
vi.mock('../../stores/desk-actions', () => ({ loadDeskFiles: vi.fn() }));
vi.mock('../../stores/channel-actions', () => ({
  loadChannels: vi.fn(),
  openChannel: vi.fn(),
  appendChannelMessage: vi.fn(),
}));
vi.mock('../../stores/preview-actions', () => ({ handleLegacyArtifactBlock: vi.fn() }));
vi.mock('../../services/app-event-actions', () => ({ handleAppEvent: vi.fn() }));
vi.mock('../../services/stream-resume', () => ({
  replayStreamResume: vi.fn(),
  isStreamResumeRebuilding: () => null,
  isStreamScopedMessage: () => false,
  updateSessionStreamMeta: vi.fn(),
}));
vi.mock('../../services/stream-key-dispatcher', () => ({ dispatchStreamKey: vi.fn() }));
vi.mock('../../hooks/use-stream-buffer', () => ({
  streamBufferManager: { handle: vi.fn(), finishRun: vi.fn() },
}));

import { handleServerMessage } from '../../services/ws-message-handler';
import { useStore } from '../../stores';
import { streamBufferManager } from '../../hooks/use-stream-buffer';

const PATH = '/session/knowledge-rollup.jsonl';

describe('knowledge_rollup_progress / knowledge_supplement_search 前端消费（2026-08-31）', () => {
  beforeEach(() => {
    useStore.setState({
      currentSessionPath: PATH,
      pendingNewSession: false,
      sessions: [{
        path: PATH,
        title: null,
        firstMessage: '',
        modified: '2026-05-08T00:00:00.000Z',
        messageCount: 0,
        agentId: 'hana',
        agentName: 'Hana',
        cwd: null,
      }],
      streamingSessions: [],
      activeSessionStreams: {},
      inlineErrors: {},
      chatSessions: {},
      knowledgeRetrievingSessions: [],
      knowledgeRollupBySession: {},
      knowledgeSupplementBySession: {},
      knowledgeTraceBySession: {},
    } as never);
  });

  it('逐轮更新滚动进度；自身事件不清除自己', () => {
    handleServerMessage({ type: 'knowledge_rollup_progress', sessionPath: PATH, current: 1, total: 3 });
    expect(useStore.getState().knowledgeRollupBySession[PATH]).toEqual({ current: 1, total: 3 });

    handleServerMessage({ type: 'knowledge_rollup_progress', sessionPath: PATH, current: 2, total: 3 });
    expect(useStore.getState().knowledgeRollupBySession[PATH]).toEqual({ current: 2, total: 3 });

    // knowledge_retrieval_started 与滚动进度同段等待：互不清除。
    handleServerMessage({ type: 'knowledge_retrieval_started', sessionPath: PATH });
    expect(useStore.getState().knowledgeRollupBySession[PATH]).toEqual({ current: 2, total: 3 });
  });

  it('补充检索事件更新查询行，且不清检索态', () => {
    handleServerMessage({ type: 'knowledge_retrieval_started', sessionPath: PATH });
    handleServerMessage({
      type: 'knowledge_supplement_search',
      sessionPath: PATH,
      queries: ['风险准备金', '交付节点'],
      round: 2,
    });
    expect(useStore.getState().knowledgeSupplementBySession[PATH]).toEqual({
      queries: ['风险准备金', '交付节点'],
      round: 2,
    });
    expect(useStore.getState().knowledgeRetrievingSessions).toContain(PATH);
  });

  it('该 session 的其他事件到达即保守清除（滚动结束）', () => {
    handleServerMessage({ type: 'knowledge_rollup_progress', sessionPath: PATH, current: 1, total: 4 });
    handleServerMessage({ type: 'knowledge_supplement_search', sessionPath: PATH, queries: ['x'], round: 1 });
    expect(useStore.getState().knowledgeRollupBySession[PATH]).toBeDefined();
    expect(useStore.getState().knowledgeSupplementBySession[PATH]).toBeDefined();

    handleServerMessage({ type: 'session_title', path: PATH, title: '新标题' });
    expect(useStore.getState().knowledgeRollupBySession[PATH]).toBeUndefined();
    expect(useStore.getState().knowledgeSupplementBySession[PATH]).toBeUndefined();
  });

  it('非法载荷（缺 sessionPath）跳过且不炸', () => {
    expect(() => handleServerMessage({ type: 'knowledge_rollup_progress', current: 1 })).not.toThrow();
    expect(() => handleServerMessage({ type: 'knowledge_supplement_search', queries: ['x'] })).not.toThrow();
    expect(useStore.getState().knowledgeRollupBySession[PATH]).toBeUndefined();
    expect(useStore.getState().knowledgeSupplementBySession[PATH]).toBeUndefined();
  });
});

describe('知识过程 → 合成工具卡（2026-08-31 四轮）', () => {
  beforeEach(() => {
    // 归零跨用例的阅读卡状态（上一用例可能残留 kt-read-* 开卡），再清 mock。
    handleServerMessage({ type: 'knowledge_retrieval_started', sessionPath: PATH });
    vi.mocked(streamBufferManager.handle).mockClear();
    useStore.setState({
      currentSessionPath: PATH,
      pendingNewSession: false,
      sessions: [{
        path: PATH,
        title: null,
        firstMessage: '',
        modified: '2026-05-08T00:00:00.000Z',
        messageCount: 0,
        agentId: 'hana',
        agentName: 'Hana',
        cwd: null,
      }],
      streamingSessions: [],
      activeSessionStreams: {},
      inlineErrors: {},
      chatSessions: {},
      knowledgeRetrievingSessions: [],
      knowledgeRollupBySession: {},
      knowledgeSupplementBySession: {},
    } as never);
  });

  it('think/search 事件各翻译成一张工具卡：start 成卡、done 收尾带结果注记', () => {
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'think-1', kind: 'think', phase: 'start' });
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'search-1', kind: 'search', phase: 'start', query: '风险准备金' });
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'search-1', kind: 'search', phase: 'done', query: '风险准备金', hits: 50 });
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'think-1', kind: 'think', phase: 'done' });

    const calls = vi.mocked(streamBufferManager.handle).mock.calls.map(call => call[0]);
    console.log('RECEIVED_CALLS', JSON.stringify(calls, null, 1));
    expect(calls).toEqual([
      { type: 'tool_start', sessionPath: PATH, id: 'kt-think-1', name: 'knowledge_think' },
      { type: 'tool_start', sessionPath: PATH, id: 'kt-search-1', name: 'knowledge_search', args: { query: '风险准备金' } },
      // resultNote 在生产经 window.t 解析为「N 个搜索结果」；测试环境无 t 回落裸 key。
      expect.objectContaining({ type: 'tool_end', id: 'kt-search-1', success: true, resultNote: expect.any(String) }),
      { type: 'tool_end', sessionPath: PATH, id: 'kt-think-1', success: true },
    ]);
  });

  it('滚动阅读：每部分一张卡，第 k 部分开始时收尾第 k-1 张', () => {
    handleServerMessage({ type: 'knowledge_rollup_progress', sessionPath: PATH, current: 1, total: 3 });
    handleServerMessage({ type: 'knowledge_rollup_progress', sessionPath: PATH, current: 2, total: 3 });

    const calls = vi.mocked(streamBufferManager.handle).mock.calls.map(call => call[0]);
    expect(calls).toEqual([
      { type: 'tool_start', sessionPath: PATH, id: 'kt-read-1', name: 'knowledge_read_part', args: { current: '1', total: '3' } },
      { type: 'tool_end', sessionPath: PATH, id: 'kt-read-1', success: true },
      { type: 'tool_start', sessionPath: PATH, id: 'kt-read-2', name: 'knowledge_read_part', args: { current: '2', total: '3' } },
    ]);
  });

  it('answer 收口：阅读卡收尾 + 「正在生成回答」卡；正文首字到达时收尾', () => {
    handleServerMessage({ type: 'knowledge_rollup_progress', sessionPath: PATH, current: 1, total: 2 });
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'answer', kind: 'note', phase: 'start', detail: 'answer' });
    handleServerMessage({ type: 'text_delta', sessionPath: PATH, streamId: 's1', delta: '答' });

    // 只断言合成卡：text_delta 本身也会正常喂缓冲（真实流事件），不属于翻译层。
    const calls = vi.mocked(streamBufferManager.handle).mock.calls
      .map(call => call[0])
      .filter(event => event.type === 'tool_start' || event.type === 'tool_end');
    expect(calls).toEqual([
      { type: 'tool_start', sessionPath: PATH, id: 'kt-read-1', name: 'knowledge_read_part', args: { current: '1', total: '2' } },
      { type: 'tool_end', sessionPath: PATH, id: 'kt-read-1', success: true },
      { type: 'tool_start', sessionPath: PATH, id: 'kt-answer', name: 'knowledge_answer' },
      { type: 'tool_end', sessionPath: PATH, id: 'kt-answer', success: true },
    ]);
  });

  it('补充检索：决策卡瞬时完成（随后真实检索各自成卡）', () => {
    handleServerMessage({ type: 'knowledge_supplement_search', sessionPath: PATH, queries: ['交付节点'], round: 2 });
    const calls = vi.mocked(streamBufferManager.handle).mock.calls.map(call => call[0]);
    expect(calls).toEqual([
      { type: 'tool_start', sessionPath: PATH, id: 'kt-supplement-2', name: 'knowledge_supplement' },
      expect.objectContaining({ type: 'tool_end', id: 'kt-supplement-2', success: true, resultNote: expect.any(String) }),
    ]);
  });

  it('非法载荷（缺 id / 缺 sessionPath）不喂卡且不炸', () => {
    expect(() => handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, kind: 'think', phase: 'start' })).not.toThrow();
    expect(() => handleServerMessage({ type: 'knowledge_trace', id: 'x', kind: 'think', phase: 'start' })).not.toThrow();
    expect(streamBufferManager.handle).not.toHaveBeenCalled();
  });
});
