// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';

const mocks = vi.hoisted(() => ({
  loadMessages: vi.fn(async () => undefined),
  // GET 反映侧边会话创建时的默认（关闭）；PATCH 回显用户选择（服务端权威）。
  lingxiFetch: vi.fn(async (path: string, opts?: RequestInit) => {
    const patched = String(path) === '/api/sessions/memory' && opts?.method === 'PATCH';
    const requested = patched ? JSON.parse(String(opts?.body ?? '{}')).memoryEnabled === true : false;
    return {
      ok: true,
      status: 200,
      statusText: 'OK',
      json: async () => ({ ok: true, sessionPath: '/session/side.jsonl', memoryEnabled: requested }),
    };
  }),
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (path: string, opts?: RequestInit) => mocks.lingxiFetch(path as never, opts as never),
  lingxiUrl: (path: string) => `http://127.0.0.1:3210${path}`,
}));

vi.mock('../../stores/session-actions', () => ({
  loadMessages: mocks.loadMessages,
  loadSessions: vi.fn(),
  loadMoreMessages: vi.fn(),
  reconcileCurrentSessionMessages: vi.fn(),
  ensureSession: vi.fn(),
  createNewSession: vi.fn(),
  continueDeletedAgentSession: vi.fn(),
  upsertOptimisticSessionFirstMessage: vi.fn(),
  fetchSessionHistoryPage: vi.fn(async () => { throw new Error('test_history_unavailable'); }),
}));

vi.mock('@/ui', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('@/ui');
  return actual;
});

import { SideChatPanel } from '../../components/side-chat/SideChatPanel';

const SIDE_PATH = '/session/side.jsonl';

function seed() {
  window.t = ((key: string) => key) as typeof window.t;
  useStore.setState({
    currentSessionPath: '/session/main.jsonl',
    currentSessionId: 'sess_main',
    currentAgentId: 'hana',
    currentTab: 'chat',
    connected: true,
    pendingNewSession: false,
    sessions: [
      { path: SIDE_PATH, sessionId: 'sess_side', agentId: 'hana', agentName: 'Hana', permissionMode: 'ask' },
      { path: '/session/main.jsonl', sessionId: 'sess_main', agentId: 'hana', agentName: 'Hana' },
    ],
    sessionLocatorsById: {
      sess_side: { path: SIDE_PATH },
      sess_main: { path: '/session/main.jsonl' },
    },
    chatSessions: {
      sess_side: { items: [], hasMore: false, loadingMore: false },
    },
    attachedFiles: [],
    attachedFilesBySession: {},
    quotedSelections: [],
    quotedSelectionsBySession: {},
    streamingSessions: [],
    turnPendingSessions: [],
    models: [],
    deskFiles: [],
  } as never);
  useStore.setState({
    sideChat: {
      open: true,
      sessionPath: SIDE_PATH,
      sessionId: 'sess_side',
      agentId: 'hana',
      status: 'ready',
      error: null,
      seedQuote: null,
      memoryEnabled: false,
      createdAt: Date.now(),
    },
  } as never);
}

describe('SideChatPanel', () => {
  beforeEach(() => {
    seed();
    mocks.loadMessages.mockClear();
    mocks.lingxiFetch.mockClear();
  });

  afterEach(() => {
    cleanup();
    useStore.getState().closeSideChat();
    useStore.getState().setCurrentSessionPathOverride(null);
  });

  it('renders the side session transcript and its own composer', async () => {
    render(React.createElement(SideChatPanel));

    expect(screen.getByRole('complementary', { name: 'sideChat.title' })).toBeTruthy();
    // 侧边会话有自己的输入区（与主对话相互独立）。
    expect(document.querySelectorAll('#inputBox').length).toBeGreaterThan(0);
    await waitFor(() => {
      expect(mocks.loadMessages).toHaveBeenCalledWith(SIDE_PATH);
    });
  });

  it('routes the shared default session to the side session while open', async () => {
    render(React.createElement(SideChatPanel));

    await waitFor(() => {
      expect(useStore.getState().currentSessionPathOverride).toBe(SIDE_PATH);
    });
    // 主会话身份没有被改动。
    expect(useStore.getState().currentSessionPath).toBe('/session/main.jsonl');
  });

  it('closes the panel and returns the shared default session to the main chat', async () => {
    render(React.createElement(SideChatPanel));

    fireEvent.click(screen.getByRole('button', { name: 'sideChat.close' }));

    expect(useStore.getState().sideChat.open).toBe(false);
    await waitFor(() => {
      expect(useStore.getState().currentSessionPathOverride).toBeNull();
    });
  });

  it('gives the side panel its own session-memory switch (off by default, toggle writes per session)', async () => {
    render(React.createElement(SideChatPanel));

    const toggle = await screen.findByRole('button', { name: 'welcome.memoryOff' });
    expect(toggle.getAttribute('data-memory-enabled')).toBe('false');

    fireEvent.click(toggle);

    await waitFor(() => {
      const patch = mocks.lingxiFetch.mock.calls.find(([path, opts]) => (
        path === '/api/sessions/memory' && (opts as RequestInit | undefined)?.method === 'PATCH'
      ));
      expect(patch).toBeTruthy();
      expect(JSON.parse(String((patch?.[1] as RequestInit).body))).toMatchObject({
        path: SIDE_PATH,
        memoryEnabled: true,
      });
    });
    // 乐观置位立即反映；服务端确认由上面的 PATCH 断言覆盖。
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'welcome.memoryOn' })).toBeTruthy();
    });
    // 读接口只按会话发起一次（缓存命中后不再重复请求）。
    expect(mocks.lingxiFetch.mock.calls.filter(([path]) => String(path).startsWith('/api/sessions/memory?'))).toHaveLength(1);
  });

  it('does not offer the main-chat "new chat" entry inside the side panel', () => {
    render(React.createElement(SideChatPanel));
    // 侧栏的语义是「在旁支里继续追问」，开新草稿入口只属于主聊天页。
    expect(screen.queryByTitle('sidebar.newChat')).toBeNull();
  });

  it('renders nothing while the panel is closed', () => {
    useStore.getState().closeSideChat();
    render(React.createElement(SideChatPanel));
    expect(document.querySelector('[data-side-chat-panel="true"]')).toBeNull();
  });
});
