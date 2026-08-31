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

import { handleServerMessage } from '../../services/ws-message-handler';
import { useStore } from '../../stores';

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

describe('knowledge_trace 过程行堆（2026-08-31 二轮）', () => {
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

  it('trace 事件按 id 追加与原位更新（start 查询词 → done 命中数）', () => {
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'think-1', kind: 'think', phase: 'start' });
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'search-1', kind: 'search', phase: 'start', query: '风险准备金' });
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'search-1', kind: 'search', phase: 'done', query: '风险准备金', hits: 50 });
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'think-1', kind: 'think', phase: 'done' });

    const trace = useStore.getState().knowledgeTraceBySession[PATH]!;
    expect(trace.map(entry => entry.id)).toEqual(['think-1', 'search-1']);
    expect(trace[0]).toMatchObject({ kind: 'think', phase: 'done' });
    expect(trace[1]).toMatchObject({ kind: 'search', phase: 'done', hits: 50 });
  });

  it('rollup/supplement 事件同步映射为 read/note 过程行', () => {
    handleServerMessage({ type: 'knowledge_rollup_progress', sessionPath: PATH, current: 2, total: 5 });
    handleServerMessage({ type: 'knowledge_supplement_search', sessionPath: PATH, queries: ['交付节点'], round: 2 });

    const trace = useStore.getState().knowledgeTraceBySession[PATH]!;
    expect(trace).toHaveLength(2);
    expect(trace[0]).toMatchObject({ id: 'read', kind: 'read', current: 2, total: 5 });
    expect(trace[1]).toMatchObject({ id: 'supplement-2', kind: 'note', queries: ['交付节点'] });
  });

  it('非法载荷（缺 id / 缺 sessionPath）跳过且不炸', () => {
    expect(() => handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, kind: 'think', phase: 'start' })).not.toThrow();
    expect(() => handleServerMessage({ type: 'knowledge_trace', id: 'x', kind: 'think', phase: 'start' })).not.toThrow();
    expect(useStore.getState().knowledgeTraceBySession[PATH]).toBeUndefined();
  });

  it('过程行堆跨真实轮事件存活，只在答案正文流式开始时收起', () => {
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'think-1', kind: 'think', phase: 'start' });
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'answer', kind: 'note', phase: 'start', detail: 'answer' });
    expect(useStore.getState().knowledgeTraceBySession[PATH]).toHaveLength(2);

    // 用户消息投影等普通事件不清过程行（等待态本身要持续到答案出现）。
    handleServerMessage({ type: 'session_title', path: PATH, title: '新标题' });
    expect(useStore.getState().knowledgeTraceBySession[PATH]).toHaveLength(2);

    // 答案正文首个 text_delta 到达 → 整堆收起。
    handleServerMessage({ type: 'text_delta', sessionPath: PATH, streamId: 's1', delta: '答' });
    expect(useStore.getState().knowledgeTraceBySession[PATH]).toBeUndefined();
  });

  it('run 结束（assistant_run_end）兜底收起；新一轮检索重开空堆', () => {
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'think-1', kind: 'think', phase: 'start' });
    handleServerMessage({ type: 'assistant_run_end', sessionPath: PATH });
    expect(useStore.getState().knowledgeTraceBySession[PATH]).toBeUndefined();

    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'search-1', kind: 'search', phase: 'start', query: 'q' });
    handleServerMessage({ type: 'knowledge_retrieval_started', sessionPath: PATH });
    expect(useStore.getState().knowledgeTraceBySession[PATH]).toBeUndefined();
  });
});

