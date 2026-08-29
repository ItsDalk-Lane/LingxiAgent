// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { UserMessage } from '../../components/chat/UserMessage';
import type { KnowledgeRetrievalStats } from '../../../../../shared/knowledge-refs.ts';
import type { ChatMessage } from '../../stores/chat-types';
import { useStore } from '../../stores';

function makeStats(partial: Partial<KnowledgeRetrievalStats>): KnowledgeRetrievalStats {
  return {
    mode: 'qa',
    retrievalMode: 'hybrid',
    subQueries: ['q1'],
    subQueryHits: [86],
    degraded: false,
    fusedChunks: 86,
    injectedChunks: 12,
    truncated: false,
    usedTokens: 920,
    budgetTokens: 4000,
    ...partial,
  };
}

function renderUserMessage(message: Partial<ChatMessage>) {
  return render(
    <UserMessage
      viewerIdentity={{ name: '小黎', avatarUrl: null }}
      isStreaming={false}
      isSelected={false}
      message={{
        id: 'u1',
        role: 'user' as const,
        text: '帮我总结路线图',
        textHtml: '<p>帮我总结路线图</p>',
        knowledgeRefs: {
          notebookIds: ['nb-1', 'nb-2'],
          mode: 'qa' as const,
          notebooks: [
            { id: 'nb-1', name: '笔记本A' },
            { id: 'nb-2', name: '笔记本B' },
          ],
        },
        ...message,
      }}
      showAvatar={false}
      sessionPath="/session/a.jsonl"
      readOnly
    />,
  );
}

describe('UserMessage knowledge meta line', () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    vi.clearAllMocks();
    window.t = ((key: string, vars?: Record<string, string | number>) => {
      const labels: Record<string, string> = {
        'chat.knowledgeMetaLabel': '知识库',
        'chat.knowledgeMetaModeQa': '问答模式',
        'chat.knowledgeMetaModeAssist': '辅助模式',
        'chat.knowledgeMetaRetrieved': '检索 {count} 块',
        'chat.knowledgeMetaInjected': '注入 {count} 块',
        'chat.knowledgeMetaTokens': '~{count} tokens',
        'chat.knowledgeMetaTruncated': '超预算分片',
        'chat.knowledgeMetaUnavailable': '知识检索不可用',
        'chat.knowledgeMetaDegradedTitle': '检索已降级：{reason}',
        'input.knowledgeModeQaHint': '严格基于检索内容回答，超出范围会明说',
        'input.knowledgeModeAssistHint': '检索内容作为参考，回答可结合对话与常识',
      };
      let text = labels[key] || key;
      for (const [name, value] of Object.entries(vars || {})) {
        text = text.replace(`{${name}}`, String(value));
      }
      return text;
    }) as typeof window.t;
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn(async () => undefined) },
    });
    useStore.setState({
      locale: 'zh',
      userAvatarUrl: null,
      userName: '小黎',
      selectedIdsBySession: {},
      streamingSessions: [],
      chatSessions: {},
    } as never);
  });

  it('renders a single muted meta line with notebook names and mode, no accent chips', () => {
    const { container } = renderUserMessage({});
    const line = screen.getByText('知识库 · 笔记本A、笔记本B · 问答模式');
    expect(line.className).toMatch(/userKnowledgeMeta/);
    // 旧版胶囊容器（AttachmentChip 排布）不再出现在引用区
    expect(container.querySelectorAll('.userAttachments')).toHaveLength(0);
    expect(line).toHaveAttribute('title', '严格基于检索内容回答，超出范围会明说');
  });

  it('falls back to raw notebook ids when names are missing', () => {
    renderUserMessage({
      knowledgeRefs: { notebookIds: ['nb-x'], mode: 'assist' },
    });
    expect(screen.getByText('知识库 · nb-x · 辅助模式')).toBeInTheDocument();
  });

  it('never appends retrieval stats to the line（统计只进折叠卡与蒸馏胶囊）', () => {
    renderUserMessage({ knowledgeRetrieval: makeStats({}) });
    const line = screen.getByText('知识库 · 笔记本A、笔记本B · 问答模式');
    expect(line.textContent).not.toContain('检索');
    expect(line.textContent).not.toContain('注入');
    expect(line.textContent).not.toContain('tokens');
    expect(line.textContent).not.toContain('超预算分片');
  });

  it('never marks over-budget truncation on the line', () => {
    renderUserMessage({ knowledgeRetrieval: makeStats({ truncated: true }) });
    expect(screen.queryByText(/超预算分片/)).not.toBeInTheDocument();
  });

  it('shows unavailable instead of numeric stats when retrieval is unavailable', () => {
    renderUserMessage({
      knowledgeRetrieval: makeStats({ unavailableReason: 'embedding store offline', fusedChunks: 0, injectedChunks: 0, usedTokens: 0 }),
    });
    const line = screen.getByText(/知识检索不可用/);
    expect(line.textContent).not.toContain('检索 0 块');
  });

  it('keeps degraded reason in the title tooltip only', () => {
    renderUserMessage({
      knowledgeRetrieval: makeStats({ degraded: true, degradeReason: '拆解失败，退回单查询' }),
    });
    const line = screen.getByText('知识库 · 笔记本A、笔记本B · 问答模式');
    expect(line).toHaveAttribute('title', expect.stringContaining('检索已降级：拆解失败，退回单查询'));
    expect(line.textContent).not.toContain('拆解失败');
  });

  it('renders no stats segment for optimistic messages without retrieval', () => {
    renderUserMessage({});
    expect(screen.getByText('知识库 · 笔记本A、笔记本B · 问答模式')).toBeInTheDocument();
    expect(screen.queryByText(/检索 \d+ 块/)).not.toBeInTheDocument();
  });
});
