// @vitest-environment jsdom
//
// ChatMessageSurface 的「等待助手 / 知识检索中 / 流式中」指示器渲染契约：
// - turnPending（发送即本地置位）→ typing 指示器立即出现（不依赖服务器信号）
// - knowledgeRetrieving → 同一指示器 + 检索中文案
// - sessionStreaming → 纯 typing 指示器；pending/检索态与其互斥切换

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../../stores';
import type { ChatListItem } from '../../../stores/chat-types';

vi.mock('../../../components/chat/ChatTranscript', () => ({
  ChatTranscript: ({ items }: { items: ChatListItem[] }) => (
    <div data-testid="transcript">
      {items.map((item) => item.type === 'message' ? <div key={item.data.id}>{item.data.id}</div> : null)}
    </div>
  ),
}));

vi.mock('../../../components/chat/ChatTimelineNavigator', () => ({
  ChatTimelineNavigator: () => null,
}));

vi.mock('../../../stores/session-actions', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../stores/session-actions')>();
  return { ...actual, loadMoreMessages: vi.fn(), reconcileCurrentSessionMessages: vi.fn() };
});

import { ChatMessageSurface } from '../../../components/chat/ChatMessageSurface';

const SESSION = '/chat/typing-indicator.jsonl';

class MockResizeObserver {
  observe() {}
  disconnect() {}
}

function message(id: string): ChatListItem {
  return { type: 'message', data: { id, role: 'user', text: `msg-${id}`, textHtml: `<p>msg-${id}</p>` } };
}

function renderSurface() {
  return render(<ChatMessageSurface sessionPath={SESSION} active />);
}

describe('ChatMessageSurface typing indicator states', () => {
  beforeEach(() => {
    window.ResizeObserver = MockResizeObserver as unknown as typeof ResizeObserver;
    useStore.setState({
      currentSessionPath: SESSION,
      currentSessionId: null,
      pendingNewSession: false,
      sessions: [],
      sessionLocatorsById: {},
      streamingSessions: [],
      activeSessionStreams: {},
      knowledgeRetrievingSessions: [],
      turnPendingSessions: [],
      unreadOutputSessionPaths: [],
      inlineErrors: {},
      chatSessions: {
        [SESSION]: { items: [message('u1')], hasMore: false, loadingMore: false, oldestId: undefined },
      },
    } as never);
  });

  afterEach(() => cleanup());

  function indicatorEl(): HTMLElement | null {
    // 测试环境 CSS module 类名带哈希（_typingIndicator_xxx），按本地名子串匹配。
    return document.querySelector('div[class*="typingIndicator"]');
  }

  it('空闲（无 pending / 检索 / 流式）不渲染指示器', () => {
    const { unmount } = renderSurface();
    expect(indicatorEl()).toBeNull();
    unmount();
  });

  it('turnPending：发送后立即渲染 typing 指示器（无检索文案）', () => {
    useStore.getState().beginTurnPending(SESSION);
    const { unmount } = renderSurface();
    const el = indicatorEl();
    expect(el).not.toBeNull();
    // 无 i18n 环境时 t() 透传 key；pending 态不带检索文案，正文为空。
    expect(el?.textContent).toBe('');
    unmount();
  });

  it('knowledgeRetrieving：指示器带检索中文案', () => {
    useStore.getState().beginKnowledgeRetrieval(SESSION);
    const { unmount } = renderSurface();
    const el = indicatorEl();
    expect(el).not.toBeNull();
    expect(el?.textContent).toBe('chat.knowledgeRetrieving');
    expect(el?.className).toContain('knowledgeRetrievingIndicator');
    unmount();
  });

  it('sessionStreaming：指示器独占，pending/检索文案不再叠加', () => {
    useStore.getState().beginTurnPending(SESSION);
    useStore.getState().beginKnowledgeRetrieval(SESSION);
    useStore.getState().addStreamingSession(SESSION);
    const { unmount } = renderSurface();
    const el = indicatorEl();
    expect(el).not.toBeNull();
    expect(el?.textContent).toBe('');
    expect(el?.className).not.toContain('knowledgeRetrievingIndicator');
    unmount();
  });
});
