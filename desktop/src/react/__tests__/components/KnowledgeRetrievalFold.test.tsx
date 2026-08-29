// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { KnowledgeRetrievalFold } from '../../components/chat/KnowledgeRetrievalFold';
import { ChatTranscript } from '../../components/chat/ChatTranscript';
import { useStore } from '../../stores';
import type { ChatListItem, ContentBlock } from '../../stores/chat-types';
import type { KnowledgeRetrievalStats } from '../../../../../shared/knowledge-refs.ts';

const sessionPath = '/session/knowledge-retrieval.jsonl';

function t(key: string, vars?: Record<string, string | number>): string {
  const table: Record<string, string> = {
    'chat.knowledgeRetrievalSearched': '已搜索 {n} 个结果',
    'chat.knowledgeRetrievalTruncated': '超预算分片',
    'chat.knowledgeRetrievalUnavailable': '知识检索不可用',
    'chat.knowledgeRetrievalRowTitle': '{source} · 块 {chunk}',
    'processFold.summary': '✨ {name}忙活了一阵子',
    'processFold.tools': '{n} 个工具',
    'processFold.thinking': '{n} 次思考',
    'processFold.unsuccessful': '{n} 次尝试未成功',
    'thinking.done': '思考完成',
  };
  return (table[key] || key).replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
}

function makeStats(partial: Partial<KnowledgeRetrievalStats> = {}): KnowledgeRetrievalStats {
  return {
    mode: 'qa',
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
    expect(summary).toHaveTextContent('1 个工具');
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
