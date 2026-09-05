// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AssistantMessage } from '../../components/chat/AssistantMessage';
import { useStore } from '../../stores';
import { clearLiveTurnStore, publishLiveAssistantMessage } from '../../stores/live-turn-store';
import { observeChatPerformance } from '../../utils/chat-performance';

vi.mock('../../utils/screenshot', () => ({
  takeScreenshot: vi.fn(),
}));

describe('AssistantMessage interlude-only rendering', () => {
  beforeEach(() => {
    window.t = ((key: string) => key) as typeof window.t;
    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });
    useStore.setState({
      agents: [],
      agentName: 'Hanako',
      agentYuan: 'lingxi',
      streamingSessions: [],
      selectedMessageIdsBySession: {},
    } as never);
  });

  afterEach(() => {
    cleanup();
    clearLiveTurnStore();
    vi.restoreAllMocks();
  });

  it('纯幕间消息不显示 Agent 身份、消息操作或完成时间', () => {
    const { container } = render(
      <AssistantMessage
        agentDisplay={{ id: 'hana', displayName: 'Hana', avatarUrl: null, fallbackAvatar: null, yuan: 'hana', isUser: false }}
        isStreaming={false}
        isSelected={false}
        showAvatar
        sessionPath="/sessions/main.jsonl"
        isLatestAssistantMessage
        message={{
          id: 'interlude-1',
          role: 'assistant',
          timestamp: Date.now(),
          blocks: [{
            type: 'interlude',
            id: 'deferred:subagent-1:success',
            variant: 'deferred_result',
            taskId: 'subagent-1',
            status: 'success',
            sourceKind: 'subagent',
            sourceLabel: '明 · 大纲评估',
            text: '小花 收到了来自 明 · 大纲评估 的回复',
            detailMarkdown: '内部详情',
          }],
        }}
      />,
    );

    expect(screen.getByText('小花 收到了来自 明 · 大纲评估 的回复')).toBeInTheDocument();
    expect(container.textContent).not.toContain('Hanako');
    expect(container.querySelector('[data-message-actions]')).toBeNull();
    expect(container.querySelector('[data-testid="assistant-completion-actions"]')).toBeNull();
  });

  it('没有最终答复时显示明确状态，而不是空白消息', () => {
    window.t = ((key: string) => ({
      'chat.turnStatus.missingFinalAnswer': '未生成最终回复',
    })[key] || key) as typeof window.t;
    render(
      <AssistantMessage
        agentDisplay={{ id: 'hana', displayName: 'Hana', avatarUrl: null, fallbackAvatar: null, yuan: 'hana', isUser: false }}
        isStreaming={false}
        isSelected={false}
        showAvatar={false}
        sessionPath="/sessions/main.jsonl"
        message={{
          id: 'assistant-no-answer',
          role: 'assistant',
          blocks: [{
            type: 'turn_status',
            status: 'missing_final_answer',
            id: 'assistant-no-answer:turn-status',
            surfaceRole: 'result',
            lifecycle: 'sealed',
          }],
        }}
      />,
    );

    expect(screen.getByText('未生成最终回复')).toBeInTheDocument();
  });

  it('实时内容只重渲染所属助手消息，不带动无关消息', () => {
    const sessionPath = '/sessions/live-overlay.jsonl';
    const agentDisplay = {
      id: 'hana',
      displayName: 'Hana',
      avatarUrl: null,
      fallbackAvatar: null,
      yuan: 'hana',
      isUser: false,
    };
    render(
      <>
        <AssistantMessage
          agentDisplay={agentDisplay}
          isStreaming
          isSelected={false}
          showAvatar={false}
          sessionPath={sessionPath}
          message={{ id: 'assistant-live', role: 'assistant', blocks: [] }}
        />
        <AssistantMessage
          agentDisplay={agentDisplay}
          isStreaming={false}
          isSelected={false}
          showAvatar={false}
          sessionPath={sessionPath}
          message={{
            id: 'assistant-history',
            role: 'assistant',
            blocks: [{ type: 'text', html: '<p>历史内容</p>', source: '历史内容' }],
          }}
        />
      </>,
    );

    const renderedMessageIds: string[] = [];
    const stop = observeChatPerformance((event) => {
      if (event.name === 'assistant_message_render' && event.messageId) {
        renderedMessageIds.push(event.messageId);
      }
    });
    act(() => {
      publishLiveAssistantMessage(sessionPath, 'assistant-live', [{
        id: 'assistant-live:text:0',
        type: 'text',
        html: '<p>实时正文</p>',
        source: '实时正文',
        semanticPhase: 'final_answer',
        surfaceRole: 'answer',
        lifecycle: 'streaming',
      }]);
    });
    stop();

    expect(screen.getByText('实时正文')).toBeInTheDocument();
    expect(screen.getByText('历史内容')).toBeInTheDocument();
    // 隔离性断言：实时块发布只影响所属消息；无关历史消息一次也不得重渲染。
    // （所属消息自身可能因外部存储一致性检查在同批多渲染一次，不影响隔离语义。）
    expect(new Set(renderedMessageIds)).toEqual(new Set(['assistant-live']));
    expect(renderedMessageIds).not.toContain('assistant-history');
  });
});
