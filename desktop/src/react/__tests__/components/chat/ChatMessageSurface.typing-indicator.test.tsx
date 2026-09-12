// @vitest-environment jsdom
//
// ChatMessageSurface 的运行状态行渲染契约：
// - turnPending（发送即本地置位）→ 状态行立即出现，尚未拿到动作时回答「正在思考中」
// - knowledgeRetrieving → 同一行切到检索中文案
// - live 快照里的在跑工具 / 未封口思考 / 流式正文 → 文案随动作切换
// - 秒表跟随状态行出现，随整数秒推进

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { act, cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../../stores';
import type { ChatListItem, ContentBlock } from '../../../stores/chat-types';
import { clearLiveTurnStore, publishLiveAssistantMessage } from '../../../stores/live-turn-store';

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

function message(id: string, role: 'user' | 'assistant' = 'user'): ChatListItem {
  return { type: 'message', data: { id, role, text: `msg-${id}`, textHtml: `<p>msg-${id}</p>` } };
}

function renderSurface() {
  return render(<ChatMessageSurface sessionPath={SESSION} active />);
}

/** 发布一条当前 session 的 live 快照，供状态行读取当前动作。 */
function publishBlocks(blocks: ContentBlock[]): void {
  publishLiveAssistantMessage(SESSION, 'a1', blocks);
}

const DURATION: Record<string, string> = {
  'duration.seconds': '{seconds}s',
  'duration.minutes': '{minutes}m {seconds}s',
  'duration.hours': '{hours}h {minutes}m {seconds}s',
  // 状态行固定文案：<助手名>正在工作中。
  'chat.running.working': '{name}正在工作中',
};

describe('ChatMessageSurface running status line', () => {
  beforeEach(() => {
    window.ResizeObserver = MockResizeObserver as unknown as typeof ResizeObserver;
    // duration 与状态行文案走语言模板；其余 chat.* 仍透传 key 便于断言。
    window.t = ((key: string, vars?: Record<string, string | number>) => {
      const template = DURATION[key];
      if (!template) return key;
      return template.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? ''));
    }) as typeof window.t;
    clearLiveTurnStore();
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
        [SESSION]: { items: [message('u1'), message('a1', 'assistant')], hasMore: false, loadingMore: false, oldestId: undefined },
      },
    } as never);
  });

  afterEach(() => {
    clearLiveTurnStore();
    cleanup();
  });

  function statusEl(): HTMLElement | null {
    return document.querySelector('[data-running-status]');
  }

  it('空闲（无 pending / 检索 / 流式）不渲染状态行', () => {
    const { unmount } = renderSurface();
    expect(statusEl()).toBeNull();
    unmount();
  });

  it('turnPending：发送后立即出现，文案是「<助手名>正在工作中」并带秒表', () => {
    useStore.getState().beginTurnPending(SESSION);
    const { unmount } = renderSurface();
    const el = statusEl();
    expect(el).not.toBeNull();
    expect(el?.textContent).toBe('Lingxi正在工作中0s');
    expect(el?.querySelector('[data-running-timer]')).not.toBeNull();
    unmount();
  });

  it('会话归属助手决定名字：agentId 命中 agents 时用该助手名', () => {
    useStore.setState({
      sessions: [{ path: SESSION, agentId: 'a-xiaoai' }],
      agents: [{ id: 'a-xiaoai', name: '小爱' }],
    } as never);
    useStore.getState().beginTurnPending(SESSION);
    const { unmount } = renderSurface();
    expect(statusEl()?.textContent).toBe('小爱正在工作中0s');
    unmount();
  });

  it('knowledgeRetrieving：文案不变，仍是正在工作中（不换成检索口吻）', () => {
    useStore.getState().beginKnowledgeRetrieval(SESSION);
    const { unmount } = renderSurface();
    const el = statusEl();
    expect(el).not.toBeNull();
    expect(el?.textContent).toBe('Lingxi正在工作中0s');
    expect(el?.getAttribute('data-knowledge-retrieving')).toBe('true');
    unmount();
  });

  it('sessionStreaming：在跑工具只影响 data-status，不改文案', () => {
    useStore.getState().addStreamingSession(SESSION);
    publishBlocks([
      { type: 'text', source: '先说一句' },
      { type: 'tool_group', collapsed: false, tools: [{ id: 't1', name: 'read', args: { path: 'a.ts' }, done: false, success: false, status: 'running' }] },
    ]);
    const { unmount } = renderSurface();
    const el = statusEl();
    expect(el?.textContent).toBe('Lingxi正在工作中0s');
    expect(el?.getAttribute('data-running-status')).toBe('reading');
    unmount();
  });

  it('思考态与纯正文态都显示同一句正在工作中', () => {
    useStore.getState().addStreamingSession(SESSION);
    publishBlocks([{ type: 'thinking', content: '想一下', sealed: false }]);
    const { unmount } = renderSurface();
    expect(statusEl()?.textContent).toBe('Lingxi正在工作中0s');
    expect(statusEl()?.getAttribute('data-running-status')).toBe('thinking');
    unmount();

    publishBlocks([{ type: 'thinking', content: '想好了', sealed: true }, { type: 'text', source: '正文' }]);
    const second = renderSurface();
    expect(statusEl()?.textContent).toBe('Lingxi正在工作中0s');
    expect(statusEl()?.getAttribute('data-running-status')).toBe('writing');
    second.unmount();
  });

  it('秒表按整数秒推进', () => {
    // 秒表读 Date.now()：假时钟必须同时接管 Date 与 setInterval。
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] });
    try {
      useStore.getState().beginTurnPending(SESSION);
      const { unmount } = renderSurface();
      expect(statusEl()?.querySelector('[data-running-timer]')?.textContent).toBe('0s');
      act(() => { vi.advanceTimersByTime(3000); });
      expect(statusEl()?.querySelector('[data-running-timer]')?.textContent).toBe('3s');
      act(() => { vi.advanceTimersByTime(62000); });
      expect(statusEl()?.querySelector('[data-running-timer]')?.textContent).toBe('1m 05s');
      unmount();
    } finally {
      vi.useRealTimers();
    }
  });
});
