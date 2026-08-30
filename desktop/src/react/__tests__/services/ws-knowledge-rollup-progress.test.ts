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
