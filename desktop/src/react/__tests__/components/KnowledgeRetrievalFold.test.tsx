// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { KnowledgeRetrievalFold } from '../../components/chat/KnowledgeRetrievalFold';
import { ChatTranscript } from '../../components/chat/ChatTranscript';
import { useStore } from '../../stores';
import type { ChatListItem, ContentBlock } from '../../stores/chat-types';
import type { KnowledgeRetrievalStats } from '../../../../../shared/knowledge-refs.ts';
import zh from '../../../locales/zh.json';
import en from '../../../locales/en.json';
import ja from '../../../locales/ja.json';
import ko from '../../../locales/ko.json';
import zhTW from '../../../locales/zh-TW.json';

const sessionPath = '/session/knowledge-retrieval.jsonl';

function t(key: string, vars?: Record<string, string | number>): string {
  const table: Record<string, string> = {
    'chat.knowledgeRetrievalSearched': '已搜索 {n} 个结果',
    'chat.knowledgeFastSummary': '快速检索 · {n} 条证据 · {ms}ms',
    'chat.knowledgeFastDeadlineExceeded': '已超出目标时限',
    'chat.knowledgeResearchSummary': '详细调查 · {rounds} 轮 · {searches} 次检索 · {reads} 次阅读 · {completed}/{total} 项完成',
    'chat.knowledgeResearchPartialSummary': '详细调查未完全完成 · {rounds} 轮 · 仍有 {pending} 项待确认',
    'chat.knowledgeResearchTruncated': '部分证据未纳入',
    'chat.knowledgeRetrievalTruncated': '超预算分片',
    'chat.knowledgeRetrievalUnavailable': '知识检索不可用',
    'chat.knowledgeRetrievalRowTitle': '{source} · 块 {chunk}',
    'chat.knowledgeRetrievalShowMore': '显示更多（还有 {n} 条）',
    'processFold.summary': '✨ {name}忙活了一阵子',
    'processFold.tools': '{n} 个工具',
    'processFold.knowledge': '{n} 次检索',
    'chat.knowledgeResearchStopRounds': '已达到调查轮数上限',
    'processFold.thinking': '{n} 次思考',
    'processFold.unsuccessful': '{n} 次尝试未成功',
    'thinking.done': '思考完成',
  };
  return (table[key] || key).replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
}

function makeStats(partial: Partial<KnowledgeRetrievalStats> = {}): KnowledgeRetrievalStats {
  return {
    mode: 'detailed',
    retrievalMode: 'hybrid',
    subQueries: ['q1'],
    subQueryHits: [8],
    degraded: false,
    fusedChunks: 8,
    injectedChunks: 2,
    truncated: false,
    usedTokens: 640,
    budgetTokens: 4000,
    results: [
      { ordinal: 1, sourceName: '笔记本A', chunkOrdinal: 3, firstLine: '第一段内容' },
      { ordinal: 2, sourceName: '笔记本B', chunkOrdinal: 1, firstLine: '第二段内容' },
    ],
    ...partial,
  };
}

function makeResearch(partial: Partial<NonNullable<KnowledgeRetrievalStats['research']>> = {}): NonNullable<KnowledgeRetrievalStats['research']> {
  return {
    runId: 'research-history', status: 'completed', completenessPolicy: 'source_diverse',
    rounds: 3, toolCalls: 17, delegatedAgents: 2, needsTotal: 4, needsSupported: 3,
    needsPartial: 0, needsConflicted: 0, unresolvedNeedIds: [], stopReason: 'complete',
    ...partial,
  };
}

function userWithStats(id: string, retrieval?: KnowledgeRetrievalStats): ChatListItem {
  return {
    type: 'message',
    data: {
      id,
      sourceEntryId: `entry-${id}`,
      role: 'user',
      text: '帮我总结路线图',
      ...(retrieval ? { knowledgeRetrieval: retrieval } : {}),
    },
  };
}

function assistant(id: string, blocks: ContentBlock[]): ChatListItem {
  return { type: 'message', data: { id, sourceEntryId: `entry-${id}`, role: 'assistant', blocks } };
}

function thinking(content = '过程思考'): ContentBlock {
  return { type: 'thinking', content, sealed: true };
}

function textBlock(html: string, source: string): ContentBlock {
  return { type: 'text', html, source };
}

function resetStore(): void {
  useStore.setState({
    agents: [],
    agentName: '小花',
    agentYuan: 'lingxi',
    streamingSessions: [],
    selectedIdsBySession: {},
    currentSessionId: null,
    currentSessionPath: sessionPath,
    sessions: [],
    sessionLocatorsById: {},
    chatSessions: { [sessionPath]: { hasMore: false, loadingMore: false, items: [] } },
    terminalsBySession: {},
    knowledgeRetrievingSessions: [],
  } as never);
}

describe('KnowledgeRetrievalFold（工具条样式检索步骤）', () => {
  beforeEach(() => {
    window.t = t as typeof window.t;
    resetStore();
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: vi.fn(),
    });
    window.requestAnimationFrame = ((callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    }) as typeof window.requestAnimationFrame;
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('收起时只显示单行摘要「已搜索 N 个结果」，结果行不渲染', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats()} />);
    expect(screen.getByText('已搜索 2 个结果')).toBeInTheDocument();
    expect(screen.queryByText(/第一段内容/)).not.toBeInTheDocument();
  });

  it('纯本地快速结果显示证据数和耗时，超过目标时限时明确标注', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats({ mode: 'fast', executionPath: 'fast_local',
      deadlineExceeded: true, truncated: true, stageTimings: { totalMs: 1201.2 } })} />);
    expect(screen.getByText('快速检索 · 2 条证据 · 1201ms')).toBeInTheDocument();
    expect(screen.getByText('已超出目标时限')).toBeInTheDocument();
    expect(screen.queryByText('超预算分片')).not.toBeInTheDocument();
  });

  it('旧快速消息没有新统计时保持原展示，不补造耗时', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats({ mode: 'fast' })} />);
    expect(screen.getByText('已搜索 2 个结果')).toBeInTheDocument();
    expect(screen.queryByText(/快速检索/)).not.toBeInTheDocument();
  });

  it('新快速消息缺失耗时时明确留空，不冒充零耗时', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats({ mode: 'fast', executionPath: 'fast_local' })} />);
    expect(screen.getByText('快速检索 · 2 条证据 · —ms')).toBeInTheDocument();
  });

  it('详细调查展示实际检索和阅读次数，并将宿主确认不适用的问题计入完成数', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats({ executionPath: 'detailed_research',
      searchCalls: 5, readCalls: 7, research: makeResearch() })} />);
    expect(screen.getByText('详细调查 · 3 轮 · 5 次检索 · 7 次阅读 · 4/4 项完成')).toBeInTheDocument();
    expect(screen.queryByText('已搜索 2 个结果')).not.toBeInTheDocument();
  });

  it('部分完成保留宿主缺口数量，不把重复需求计成多个缺口', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats({ research: makeResearch({ status: 'partial',
      needsSupported: 1, needsPartial: 1, needsConflicted: 1,
      unresolvedNeedIds: ['need-partial', 'need-conflict', 'need-partial'], stopReason: 'round_budget_exhausted' }) })} />);
    expect(screen.getByText('详细调查未完全完成 · 3 轮 · 仍有 2 项待确认 · 已达到调查轮数上限')).toBeInTheDocument();
    expect(screen.queryByText(/4\/4 项完成/)).not.toBeInTheDocument();
  });

  it('已完成调查仍按实际需求进度展示，不把未确认的可选项冒充完成', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats({ searchCalls: 5, readCalls: 7,
      research: makeResearch({ needsSupported: 2, unresolvedNeedIds: ['optional-need'] }) })} />);
    expect(screen.getByText('详细调查 · 3 轮 · 5 次检索 · 7 次阅读 · 3/4 项完成')).toBeInTheDocument();
  });

  it('缺失或非法研究统计保留未知，不借用工具总数或证据条数补齐', () => {
    const research = { ...makeResearch(), rounds: Number.NaN, needsTotal: undefined,
      unresolvedNeedIds: undefined } as unknown as NonNullable<KnowledgeRetrievalStats['research']>;
    render(<KnowledgeRetrievalFold retrieval={makeStats({ searchCalls: undefined, readCalls: -1, research })} />);
    expect(screen.getByText('详细调查 · ? 轮 · ? 次检索 · ? 次阅读 · ?/? 项完成')).toBeInTheDocument();
  });

  it('部分结果缺失缺口清单时不宣称零项待确认', () => {
    const research = { ...makeResearch({ status: 'partial' }), unresolvedNeedIds: undefined } as unknown as NonNullable<KnowledgeRetrievalStats['research']>;
    render(<KnowledgeRetrievalFold retrieval={makeStats({ research })} />);
    expect(screen.getByText('详细调查未完全完成 · 3 轮 · 仍有 ? 项待确认')).toBeInTheDocument();
  });

  it('已保存的零次调用保持零次，不当作缺失', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats({ searchCalls: 0, readCalls: 0,
      research: makeResearch({ rounds: 0 }) })} />);
    expect(screen.getByText('详细调查 · 0 轮 · 0 次检索 · 0 次阅读 · 4/4 项完成')).toBeInTheDocument();
  });

  it('调查证据截断时说明部分证据未纳入，不声称旧流程的分片阅读', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats({ truncated: true, searchCalls: 5,
      readCalls: 7, research: makeResearch() })} />);
    expect(screen.getByText('详细调查 · 3 轮 · 5 次检索 · 7 次阅读 · 4/4 项完成')).toBeInTheDocument();
    expect(screen.getByText('部分证据未纳入')).toBeInTheDocument();
    expect(screen.queryByText('超预算分片')).not.toBeInTheDocument();
  });

  it('退出重进后只用持久化消息恢复调查摘要和证据列表', async () => {
    const savedItems = JSON.stringify([
      userWithStats('research-u1', makeStats({ executionPath: 'detailed_research', searchCalls: 5,
        readCalls: 7, research: makeResearch() })),
      assistant('research-a1', [textBlock('<p>调查答复</p>', '调查答复')]),
    ]);
    const history = () => <ChatTranscript items={JSON.parse(savedItems)} sessionPath={sessionPath} />;
    const firstVisit = render(history());
    expect(screen.getByText('详细调查 · 3 轮 · 5 次检索 · 7 次阅读 · 4/4 项完成')).toBeInTheDocument();
    firstVisit.unmount();
    resetStore();
    render(history());
    expect(screen.getAllByTestId('knowledge-retrieval-fold')).toHaveLength(1);
    fireEvent.click(screen.getByText('详细调查 · 3 轮 · 5 次检索 · 7 次阅读 · 4/4 项完成'));
    await waitFor(() => expect(screen.getByText('第一段内容')).toBeInTheDocument());
    expect(screen.getByText('调查答复')).toBeInTheDocument();
    expect(screen.queryByText('research-history')).not.toBeInTheDocument();
  });

  it.each([
    ['zh', zh, '详细调查 · 3 轮 · 5 次检索 · 7 次阅读 · 4/4 项完成', '详细调查未完全完成 · 3 轮 · 仍有 2 项待确认'],
    ['en', en, 'Detailed research · 3 rounds · 5 searches · 7 reads · 4/4 items complete', 'Detailed research partially complete · 3 rounds · 2 items still unconfirmed'],
    ['ja', ja, '詳細調査 · 3 ラウンド · 検索 5 回 · 閲読 7 回 · 4/4 項目完了', '詳細調査は一部未完了 · 3 ラウンド · 未確認 2 項目'],
    ['ko', ko, '상세 조사 · 3라운드 · 검색 5회 · 읽기 7회 · 4/4개 항목 완료', '상세 조사 일부 미완료 · 3라운드 · 2개 항목 확인 필요'],
    ['zh-TW', zhTW, '詳細調查 · 3 輪 · 5 次檢索 · 7 次閱讀 · 4/4 項完成', '詳細調查尚未完全完成 · 3 輪 · 仍有 2 項待確認'],
  ] as const)('%s 使用真实语言包展示完成和部分结果', (_locale, dictionary, completed, partial) => {
    window.t = ((key: string, vars?: Record<string, string | number>) => {
      const template = dictionary.chat[key.slice('chat.'.length) as keyof typeof dictionary.chat];
      return (typeof template === 'string' ? template : key).replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
    }) as typeof window.t;
    const { rerender } = render(<KnowledgeRetrievalFold retrieval={makeStats({ searchCalls: 5, readCalls: 7, research: makeResearch() })} />);
    expect(screen.getByText(completed)).toBeInTheDocument();
    rerender(<KnowledgeRetrievalFold retrieval={makeStats({ research: makeResearch({ status: 'partial', unresolvedNeedIds: ['n1', 'n2'] }) })} />);
    expect(screen.getByText(partial)).toBeInTheDocument();
  });

  it('点击展开后逐行显示 #编号 + 首行，行 title 悬浮 source · 块序号', async () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats()} />);
    fireEvent.click(screen.getByText('已搜索 2 个结果'));
    await waitFor(() => expect(screen.getByText('#1')).toBeInTheDocument());
    const firstRow = screen.getByText('第一段内容').closest('div');
    expect(firstRow).toHaveAttribute('title', '笔记本A · 块 3');
    expect(screen.getByText('第二段内容').closest('div')).toHaveAttribute('title', '笔记本B · 块 1');
    // 编号与首行同行（同一 toolIndicator 行内）
    const ordinalRow = screen.getByText('#1').closest('div');
    expect(within(ordinalRow as HTMLElement).getByText('第一段内容')).toBeInTheDocument();
  });

  it('二次展开：首屏只渲染 10 条，「显示更多」一次性放出剩余', async () => {
    const results = Array.from({ length: 35 }, (_, index) => ({
      ordinal: index + 1,
      sourceName: '笔记本A',
      chunkOrdinal: index + 1,
      firstLine: `证据行${index + 1}`,
    }));
    render(<KnowledgeRetrievalFold retrieval={makeStats({ results, injectedChunks: 35 })} />);
    fireEvent.click(screen.getByText('已搜索 35 个结果'));
    await waitFor(() => expect(screen.getByText('#1')).toBeInTheDocument());
    // 首屏 = 前 10 条（证据行1~10），第 11 条不可见。
    expect(screen.getAllByText(/^证据行\d+$/).length).toBe(10);
    expect(screen.queryByText('证据行11')).not.toBeInTheDocument();
    // 「显示更多（还有 25 条）」二级展开：点击后剩余全部放出。
    fireEvent.click(screen.getByTestId('knowledge-retrieval-show-more'));
    await waitFor(() => expect(screen.getByText('证据行35')).toBeInTheDocument());
    expect(screen.getAllByText(/^证据行\d+$/).length).toBe(35);
    expect(screen.queryByTestId('knowledge-retrieval-show-more')).not.toBeInTheDocument();
  });

  it('结果 ≤ 10 条时无「显示更多」按钮（一次性全展开）', async () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats()} />);
    fireEvent.click(screen.getByText('已搜索 2 个结果'));
    await waitFor(() => expect(screen.getByText('#1')).toBeInTheDocument());
    expect(screen.queryByTestId('knowledge-retrieval-show-more')).not.toBeInTheDocument();
    expect(screen.getByText('第二段内容')).toBeInTheDocument();
  });

  it('truncated 时摘要行追加「超预算分片」小标', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats({ truncated: true })} />);
    expect(screen.getByText('已搜索 2 个结果')).toBeInTheDocument();
    expect(screen.getByText('超预算分片')).toBeInTheDocument();
  });

  it('unavailableReason 时只报「知识检索不可用」，不可展开且无结果行', () => {
    render(<KnowledgeRetrievalFold retrieval={makeStats({ unavailableReason: 'engine unavailable', results: undefined })} />);
    expect(screen.getByText('知识检索不可用')).toBeInTheDocument();
    expect(screen.queryByText(/已搜索/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('知识检索不可用'));
    expect(screen.queryByText(/第一段内容/)).not.toBeInTheDocument();
  });

  it('ProcessFold 路径：检索步骤收进收纳块，摘要计数 +1，展开后可见', async () => {
    render(
      <ChatTranscript
        items={[
          userWithStats('u1', makeStats()),
          assistant('a1', [thinking('想了想'), textBlock('<p>最终答复</p>', '最终答复')]),
        ]}
        sessionPath={sessionPath}
        enableProcessFold
      />,
    );

    // 检索算一步：0 工具 + 1 知识检索 → 「1 个工具」
    const summary = screen.getByRole('button', { name: /小花忙活了一阵子/ });
    expect(summary).toHaveTextContent('1 次检索');
    expect(screen.queryByTestId('knowledge-retrieval-fold')).not.toBeInTheDocument();

    fireEvent.click(summary);
    await waitFor(() => expect(screen.getByTestId('knowledge-retrieval-fold')).toBeInTheDocument());
    expect(screen.getByText('已搜索 2 个结果')).toBeInTheDocument();
    expect(screen.getByText('最终答复')).toBeInTheDocument();
  });

  it('非 ProcessFold 路径：卡片直接渲染在 assistant 回复最前', () => {
    render(
      <ChatTranscript
        items={[
          userWithStats('u1', makeStats()),
          assistant('a1', [textBlock('<p>最终答复</p>', '最终答复')]),
        ]}
        sessionPath={sessionPath}
      />,
    );

    expect(screen.getByTestId('knowledge-retrieval-fold')).toBeInTheDocument();
    expect(screen.getByText('已搜索 2 个结果')).toBeInTheDocument();
    expect(screen.getByText('最终答复')).toBeInTheDocument();
  });

  it('配对 user 消息无检索统计时不渲染卡片，摘要计数不变', () => {
    const { container } = render(
      <ChatTranscript
        items={[
          userWithStats('u1'),
          assistant('a1', [thinking('想了想'), textBlock('<p>最终答复</p>', '最终答复')]),
        ]}
        sessionPath={sessionPath}
        enableProcessFold
      />,
    );

    expect(container.querySelectorAll('[data-testid="knowledge-retrieval-fold"]')).toHaveLength(0);
    // 0 工具 + 0 知识检索 → 摘要只有思考计数
    expect(screen.getByRole('button', { name: /小花忙活了一阵子/ })).toHaveTextContent('1 次思考');
    expect(screen.getByRole('button', { name: /小花忙活了一阵子/ })).not.toHaveTextContent('个工具');
  });

  it('一轮多条 assistant 消息只渲染一张检索卡', async () => {
    render(
      <ChatTranscript
        items={[
          userWithStats('u1', makeStats()),
          assistant('a1', [thinking('第一步')]),
          assistant('a2', [textBlock('<p>最终答复</p>', '最终答复')]),
        ]}
        sessionPath={sessionPath}
        enableProcessFold
      />,
    );

    const summary = screen.getByRole('button', { name: /小花忙活了一阵子/ });
    fireEvent.click(summary);
    await waitFor(() => expect(screen.getAllByTestId('knowledge-retrieval-fold')).toHaveLength(1));
  });
});
