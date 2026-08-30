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

const PATH = '/session/knowledge-distill.jsonl';

describe('knowledge_distill_progress 前端消费', () => {
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
      knowledgeDistillBySession: {},
    } as never);
  });

  it('逐批更新胶囊计数；自身事件不清除自己', () => {
    handleServerMessage({ type: 'knowledge_distill_progress', sessionPath: PATH, done: 5, model: 'p/m' });
    expect(useStore.getState().knowledgeDistillBySession[PATH]).toEqual({ model: 'p/m', done: 5 });

    handleServerMessage({ type: 'knowledge_distill_progress', sessionPath: PATH, done: 6, model: 'p/m' });
    expect(useStore.getState().knowledgeDistillBySession[PATH]).toEqual({ model: 'p/m', done: 6 });

    // knowledge_retrieval_started 与 distill 进度同段等待：互不清除。
    handleServerMessage({ type: 'knowledge_retrieval_started', sessionPath: PATH });
    expect(useStore.getState().knowledgeDistillBySession[PATH]).toEqual({ model: 'p/m', done: 6 });
  });

  it('该 session 的其他事件到达即保守清除（蒸馏结束）', () => {
    handleServerMessage({ type: 'knowledge_distill_progress', sessionPath: PATH, done: 3, model: 'p/m' });
    expect(useStore.getState().knowledgeDistillBySession[PATH]).toBeDefined();

    handleServerMessage({ type: 'session_title', path: PATH, title: '新标题' });
    expect(useStore.getState().knowledgeDistillBySession[PATH]).toBeUndefined();
  });

  it('非法载荷（缺 sessionPath）跳过且不炸', () => {
    expect(() => handleServerMessage({ type: 'knowledge_distill_progress', done: 1 })).not.toThrow();
    expect(useStore.getState().knowledgeDistillBySession[PATH]).toBeUndefined();
  });

  it('knowledge_coverage_progress（Phase 9 第二波）：不清检索态、不炸（渲染留给后续版本）', () => {
    handleServerMessage({ type: 'knowledge_retrieval_started', sessionPath: PATH });
    expect(useStore.getState().knowledgeRetrievingSessions).toContain(PATH);

    // 覆盖进度是检索期内的分段进度：不提前清除「检索中」胶囊，也不产生渲染状态。
    expect(() => handleServerMessage({
      type: 'knowledge_coverage_progress',
      sessionPath: PATH,
      runId: 'covrun_1',
      done: 3,
      total: 8,
    })).not.toThrow();
    expect(useStore.getState().knowledgeRetrievingSessions).toContain(PATH);
  });
});
