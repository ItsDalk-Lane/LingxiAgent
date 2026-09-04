import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../stores/session-actions', () => ({ loadSessions: vi.fn() }));
vi.mock('../../stores/channel-actions', () => ({ loadChannels: vi.fn(), openChannel: vi.fn(), appendChannelMessage: vi.fn() }));
vi.mock('../../stores/preview-actions', () => ({ handleLegacyArtifactBlock: vi.fn() }));
vi.mock('../../services/app-event-actions', () => ({ handleAppEvent: vi.fn() }));
vi.mock('../../services/stream-resume', () => ({ replayStreamResume: vi.fn(), isStreamResumeRebuilding: () => null,
  isStreamScopedMessage: () => false, updateSessionStreamMeta: vi.fn() }));
vi.mock('../../services/stream-key-dispatcher', () => ({ dispatchStreamKey: vi.fn() }));

import { handleServerMessage } from '../../services/ws-message-handler';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import { useStore } from '../../stores';
import { getToolLabel } from '../../utils/tool-label';
import zh from '../../../locales/zh.json';
import type { KnowledgeResearchProgress } from '../../../../../shared/knowledge-research.ts';

const PATH = '/session/research-progress.jsonl';
const OTHER = '/session/research-other.jsonl';

function translate(key: string, vars: Record<string, string> = {}): string {
  const value = key.split('.').reduce<unknown>((node, part) => node && typeof node === 'object'
    ? (node as Record<string, unknown>)[part] : undefined, zh);
  return (typeof value === 'string' ? value : key).replace(/\{(\w+)\}/g, (match, name) => vars[name] ?? match);
}

function send(type: KnowledgeResearchProgress['type'], extra: Record<string, unknown> = {}, sessionPath = PATH) {
  handleServerMessage({ type, sessionPath, runId: 'run', scopeId: 'scope', rounds: 0, maxRounds: 4,
    searchCalls: 0, readCalls: 0, delegatedAgents: 0, needsTotal: 3, needsSupported: 0, needsPartial: 0,
    needsConflicted: 0, unresolvedNeedIds: ['n1', 'n2', 'n3'], ...extra });
}

function cards(sessionPath = PATH) {
  return streamBufferManager.snapshot(sessionPath)?.blocks?.flatMap(block => block.type === 'tool_group' ? block.tools : []) ?? [];
}

function committedCards() {
  return useStore.getState().chatSessions[PATH].items.flatMap(item => item.type === 'message'
    ? (item.data.blocks || []).flatMap(block => block.type === 'tool_group' ? block.tools : []) : []);
}

function begin(sessionPath = PATH) {
  send('knowledge_research_started', {}, sessionPath);
  send('knowledge_research_round_started', { roundId: 'round-1', round: 1 }, sessionPath);
}

beforeEach(() => {
  for (const sessionPath of [PATH, OTHER]) handleServerMessage({ type: 'knowledge_retrieval_started', sessionPath });
  streamBufferManager.clearAll();
  useStore.setState({ currentSessionId: null, currentSessionPath: PATH, sessionLocatorsById: {},
    sessions: [{ path: PATH }, { path: OTHER }], pendingNewSession: false, chatSessions: {},
    streamingSessions: [], activeSessionStreams: {}, knowledgeRetrievingSessions: [] } as never);
  for (const sessionPath of [PATH, OTHER]) useStore.getState().initSession(sessionPath, [], false);
  vi.stubGlobal('window', { t: translate });
});
afterEach(() => { streamBufferManager.clearAll(); vi.unstubAllGlobals(); });

describe('研究实时进度使用真实聊天缓冲', () => {
  it('七类事件形成规划、轮次、每任务一张卡、证据进度、核对和整理，并原位更新完成数', () => {
    send('knowledge_research_started'); send('knowledge_research_started');
    send('knowledge_research_plan_updated', { hiddenReasoning: '禁止展示的隐藏推理', toolOutput: { body: '原始工具正文' } });
    send('knowledge_research_plan_updated');
    send('knowledge_research_round_started', { roundId: 'round-1', round: 1 });
    send('knowledge_research_round_started', { roundId: 'round-1', round: 1 });
    for (const taskId of ['task-a', 'task-b']) {
      send('knowledge_research_worker_started', { taskId, label: `调查${taskId}`, delegatedAgents: 2 });
      send('knowledge_research_worker_started', { taskId, label: `调查${taskId}`, delegatedAgents: 2 });
    }
    send('knowledge_research_ledger_updated', { phase: 'investigating', needsSupported: 1, unresolvedNeedIds: ['n2', 'n3'] });
    const id = cards().find(card => card.name === 'knowledge_research_progress')!.id;
    send('knowledge_research_ledger_updated', { phase: 'investigating', needsSupported: 1, unresolvedNeedIds: ['n3', 'n3'] });
    // 宿主认定不适用的需求也算已处理；完成数不能直接取支持数。
    expect(cards().find(card => card.id === id)?.args).toEqual({ completed: 2, total: 3 });
    for (const taskId of ['task-a', 'task-b']) {
      send('knowledge_research_worker_completed', { taskId, label: `调查${taskId}`, status: 'completed' });
      send('knowledge_research_worker_completed', { taskId, label: `调查${taskId}`, status: 'completed' });
    }
    send('knowledge_research_ledger_updated', { phase: 'reviewing', needsConflicted: 1, unresolvedNeedIds: ['n3'] });
    send('knowledge_research_completed', { status: 'partial', rounds: 1, stopReason: 'max_rounds', unresolvedNeedIds: ['n3'] });
    expect(cards().map(card => card.name)).toEqual(['knowledge_research_plan', 'knowledge_research_round',
      'knowledge_research_worker', 'knowledge_research_worker', 'knowledge_research_progress', 'knowledge_research_review', 'knowledge_research_synthesis']);
    expect(cards().filter(card => !card.done).map(card => card.name)).toEqual(['knowledge_research_synthesis']);
    expect(cards().find(card => card.name === 'knowledge_research_review')?.resultNote).toBe('1 项矛盾 · 1 项待确认');
    expect(cards().at(-1)?.resultNote).toBe('详细调查未完全完成 · 1 轮 · 仍有 1 项待确认');
    expect(JSON.stringify(cards())).not.toContain('隐藏推理');
    expect(JSON.stringify(cards())).not.toContain('原始工具正文');
    expect(getToolLabel('knowledge_research_round', 'running', '灵犀', { round: 1, maxRounds: 4 })).toBe('第 1/4 轮：正在检索和阅读');
  });

  it('新一轮收尾旧轮，旧轮重复和同任务迟到的开始事件不重新开卡', () => {
    begin();
    send('knowledge_research_plan_updated');
    send('knowledge_research_worker_completed', { taskId: 'task-a', label: '核对日期', status: 'completed' });
    send('knowledge_research_worker_started', { taskId: 'task-a', label: '核对日期' });
    send('knowledge_research_round_started', { roundId: 'round-2', round: 2 });
    send('knowledge_research_round_started', { roundId: 'round-1', round: 1 });
    expect(cards().filter(card => card.name === 'knowledge_research_round')).toHaveLength(2);
    expect(cards().filter(card => !card.done).map(card => card.args?.round)).toEqual([2]);
    expect(cards().filter(card => card.name === 'knowledge_research_worker')).toHaveLength(1);
  });

  it('真实先开轮再建需求的顺序中，规划卡等计划到达才完成并显示最新计数', () => {
    begin();
    expect(cards().find(card => card.name === 'knowledge_research_plan')?.done).toBe(false);
    send('knowledge_research_plan_updated', { needsTotal: 2, unresolvedNeedIds: ['n1', 'n2'] });
    const plan = cards().find(card => card.name === 'knowledge_research_plan');
    expect(plan).toMatchObject({ done: true, status: 'succeeded', resultNote: '已完成 0/2 个证据问题' });
    send('knowledge_research_plan_updated', { needsTotal: 3, unresolvedNeedIds: ['n1', 'n2', 'n3'] });
    expect(cards().filter(card => card.name === 'knowledge_research_plan')).toHaveLength(1);
    expect(cards().find(card => card.id === plan?.id)?.resultNote).toBe('已完成 0/3 个证据问题');
    expect(useStore.getState().knowledgeRetrievingSessions).toContain(PATH);
  });

  it('追加消息调查期间旧主回答的正文不会提前结束研究卡', () => {
    begin();
    handleServerMessage({ type: 'text_delta', sessionPath: PATH, delta: '原回答仍在继续' });
    expect(cards().filter(card => !card.done)).toHaveLength(2);
    send('knowledge_research_plan_updated');
    send('knowledge_research_worker_started', { taskId: 'task-a', label: '核对日期' });
    expect(cards().find(card => card.name === 'knowledge_research_worker')?.done).toBe(false);
  });

  it('旧主回答结束后仍按原id更新研究卡，既不重复卡也不制造空助手消息', () => {
    handleServerMessage({ type: 'assistant_run_start', sessionPath: PATH, runId: 'previous-main' });
    handleServerMessage({ type: 'text_delta', sessionPath: PATH, delta: '旧主回答' });
    begin(); send('knowledge_research_plan_updated');
    send('knowledge_research_worker_started', { taskId: 'task-a', label: '核对日期' });
    const worker = cards().find(card => card.name === 'knowledge_research_worker')!;
    handleServerMessage({ type: 'assistant_run_end', sessionPath: PATH, runId: 'previous-main', assistantEntryId: 'previous-answer' });
    const before = useStore.getState().chatSessions[PATH].items;
    expect(committedCards().find(card => card.id === worker.id)?.status).toBe('running');
    send('knowledge_research_worker_completed', { taskId: 'task-a', label: '核对日期', status: 'completed' });
    const after = useStore.getState().chatSessions[PATH].items;
    expect(after).not.toBe(before);
    expect(after).toHaveLength(before.length);
    expect(committedCards().filter(card => card.id === worker.id)).toHaveLength(1);
    expect(committedCards().find(card => card.id === worker.id)).toMatchObject({ status: 'succeeded', done: true });
    expect(cards()).toEqual([]);
  });

  it('调查完成后旧主回答才结束时，整理卡仍等待本次回答再收尾', () => {
    handleServerMessage({ type: 'assistant_run_start', sessionPath: PATH, runId: 'previous-main' });
    begin(); send('knowledge_research_completed', { status: 'completed', stopReason: 'complete', unresolvedNeedIds: [] });
    const synthesis = cards().find(card => card.name === 'knowledge_research_synthesis')!;
    handleServerMessage({ type: 'text_delta', sessionPath: PATH, delta: '上一条回答的结尾' });
    expect(cards().find(card => card.id === synthesis.id)?.done).toBe(false);
    handleServerMessage({ type: 'assistant_run_end', sessionPath: PATH, runId: 'previous-main', assistantEntryId: 'previous-answer' });
    expect(committedCards().find(card => card.id === synthesis.id)?.done).toBe(false);
    handleServerMessage({ type: 'assistant_run_start', sessionPath: PATH, runId: 'new-main' });
    handleServerMessage({ type: 'text_delta', sessionPath: PATH, delta: '新回答' });
    expect(committedCards().find(card => card.id === synthesis.id)?.status).toBe('succeeded');
    expect(committedCards().filter(card => card.id === synthesis.id)).toHaveLength(1);
  });

  for (const status of ['cancelled', 'failed'] as const) {
    it(`${status}收尾所有在途卡，重复终态和迟到事件不新增卡`, () => {
      begin(); send('knowledge_research_worker_started', { taskId: 'task-a', label: '调查任务' });
      send('knowledge_research_ledger_updated', { phase: 'investigating' });
      send('knowledge_research_completed', { status, stopReason: status });
      const snapshot = cards();
      expect(snapshot.every(card => card.done)).toBe(true);
      expect(snapshot.filter(card => card.name !== 'knowledge_research_plan').every(card => card.status === 'failed')).toBe(true);
      expect(snapshot.some(card => card.name === 'knowledge_research_synthesis')).toBe(false);
      expect(useStore.getState().knowledgeRetrievingSessions).not.toContain(PATH);
      send('knowledge_research_completed', { status, stopReason: status });
      send('knowledge_research_worker_started', { taskId: 'late-task', label: '迟到任务' });
      send('knowledge_research_started');
      handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'answer', detail: 'answer' });
      expect(cards()).toEqual(snapshot);
    });
  }

  for (const status of ['completed', 'partial'] as const) {
    it(`${status}整理卡和旧回答通知共用一次等待，正文首字收尾且不创建额外助手消息`, () => {
      begin(); send('knowledge_research_completed', { status, stopReason: status, unresolvedNeedIds: [] });
      send('knowledge_research_completed', { status, stopReason: status, unresolvedNeedIds: [] });
      handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'answer', detail: 'answer' });
      handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'answer', detail: 'answer' });
      expect(cards().filter(card => card.name === 'knowledge_research_synthesis')).toHaveLength(1);
      expect(cards().some(card => card.name === 'knowledge_answer')).toBe(false);
      handleServerMessage({ type: 'text_delta', sessionPath: PATH, delta: '答' });
      expect(cards().every(card => card.done)).toBe(true);
      handleServerMessage({ type: 'text_delta', sessionPath: PATH, delta: '案' });
      const items = useStore.getState().chatSessions[PATH].items;
      expect(items.filter(item => item.type === 'message' && item.data.role === 'assistant')).toHaveLength(1);
    });
  }

  it('会话各自维护相同run和task的卡片，停止一边不影响另一边', () => {
    for (const sessionPath of [PATH, OTHER]) {
      begin(sessionPath); send('knowledge_research_plan_updated', {}, sessionPath);
      send('knowledge_research_worker_started', { taskId: 'same-task', label: '资料核对' }, sessionPath);
    }
    send('knowledge_research_completed', { status: 'cancelled', stopReason: 'cancelled' });
    expect(cards().every(card => card.done)).toBe(true);
    expect(cards(OTHER).filter(card => !card.done)).toHaveLength(2);
    send('knowledge_research_worker_completed', { taskId: 'same-task', label: '资料核对', status: 'completed' }, OTHER);
    expect(cards(OTHER).filter(card => !card.done)).toHaveLength(1);
  });

  it('未知运行、范围不符和缺失计数不绘制虚假进度', () => {
    send('knowledge_research_round_started', { roundId: 'orphan', round: 1 });
    send('knowledge_research_started', { needsTotal: undefined });
    expect(cards()).toEqual([]);
    begin(); const before = cards();
    send('knowledge_research_ledger_updated', { phase: 'investigating', scopeId: 'different-scope' });
    send('knowledge_research_ledger_updated', { phase: 'investigating', runId: 'different-run' });
    send('knowledge_research_worker_started', { taskId: 'task-a', label: '字'.repeat(101) });
    expect(cards()).toEqual(before);
  });

  it('开始下一次快速检索后恢复原回答卡，不继承上一轮详细模式的去重状态', () => {
    begin(); send('knowledge_research_completed', { status: 'completed', stopReason: 'complete', unresolvedNeedIds: [] });
    handleServerMessage({ type: 'knowledge_retrieval_started', sessionPath: PATH });
    handleServerMessage({ type: 'knowledge_trace', sessionPath: PATH, id: 'answer', detail: 'answer' });
    expect(cards().filter(card => card.name === 'knowledge_answer')).toHaveLength(1);
  });

  it('内部计数更新不创建无主卡，旧工具重复start仍不覆盖原参数', () => {
    streamBufferManager.updateKnowledgeResearchToolProgress(PATH, 'missing', { count: 2 });
    expect(useStore.getState().chatSessions[PATH].items).toHaveLength(0);
    streamBufferManager.handle({ type: 'tool_start', sessionPath: PATH, id: 'legacy', name: 'read', args: { path: '/first' } });
    streamBufferManager.handle({ type: 'tool_start', sessionPath: PATH, id: 'legacy', name: 'read', args: { path: '/second' } });
    expect(cards()).toHaveLength(1); expect(cards()[0].args).toEqual({ path: '/first' });
  });
});
