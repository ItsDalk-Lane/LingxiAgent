/**
 * @vitest-environment jsdom
 */
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const lingxiFetchMock = vi.fn();
const switchSessionMock = vi.fn();
const locateSearchHitMock = vi.fn();

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (...args: unknown[]) => lingxiFetchMock(...args),
  lingxiUrl: (p: string) => p,
}));

vi.mock('../../stores/session-actions', () => ({
  switchSession: (...args: unknown[]) => switchSessionMock(...args),
}));

vi.mock('../../stores/chat-find-actions', () => ({
  locateSearchHit: (...args: unknown[]) => locateSearchHitMock(...args),
}));

vi.mock('../../hooks/use-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}));

import { ChatSearchOverlay } from '../../components/search/ChatSearchOverlay';
import { useStore } from '../../stores';

function jsonResponse(data: unknown) {
  return { json: async () => data };
}

function seedSessions() {
  useStore.setState({
    sessions: [
      {
        path: '/tmp/agents/hana/sessions/a.jsonl',
        sessionId: 'sess_a',
        title: 'Alpha chat',
        firstMessage: 'hello',
        modified: new Date().toISOString(),
        messageCount: 2,
        agentId: 'hana',
        agentName: 'Hana',
        cwd: '/tmp/workspace-a',
        pinnedAt: null,
        hasSummary: false,
      },
      {
        path: '/tmp/agents/hana/sessions/b.jsonl',
        sessionId: 'sess_b',
        title: 'Beta chat',
        firstMessage: 'hello',
        modified: new Date(Date.now() - 86_400_000).toISOString(),
        messageCount: 2,
        agentId: 'hana',
        agentName: 'Hana',
        cwd: '/tmp/workspace-b',
        pinnedAt: '2026-01-01T00:00:00.000Z',
        pinOrder: 1,
        hasSummary: false,
      },
    ],
    currentSessionPath: '/tmp/agents/hana/sessions/a.jsonl',
    pendingSessionSwitchPath: null,
    pendingNewSession: false,
    agents: [],
    chatSearchOpen: true,
  } as never);
}

describe('ChatSearchOverlay', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    lingxiFetchMock.mockReset();
    lingxiFetchMock.mockImplementation(async () => jsonResponse({ results: [] }));
    switchSessionMock.mockReset();
    locateSearchHitMock.mockReset();
    seedSessions();
  });

  afterEach(() => {
    vi.useRealTimers();
    cleanup();
    useStore.setState({ chatSearchOpen: false } as never);
  });

  it('renders nothing while closed', () => {
    useStore.setState({ chatSearchOpen: false } as never);
    const { container } = render(<ChatSearchOverlay />);
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByTestId('chat-search-overlay')).not.toBeInTheDocument();
  });

  it('shows every session globally on an empty query, ignoring workspace scoping', () => {
    render(<ChatSearchOverlay />);

    const overlay = screen.getByRole('dialog');
    expect(overlay).toBeInTheDocument();
    // 两个不同 workspace（含 pinned）的会话都出现 —— 全局列表，不受当前工作台限制。
    expect(screen.getByText('Alpha chat')).toBeInTheDocument();
    expect(screen.getByText('Beta chat')).toBeInTheDocument();
    expect(screen.getByText('sidebar.pinned')).toBeInTheDocument();
    expect(lingxiFetchMock).not.toHaveBeenCalledWith(
      expect.stringContaining('/api/sessions/search'),
      expect.anything(),
    );
  });

  it('runs the two-phase search (title then content) and shows sections', async () => {
    lingxiFetchMock.mockImplementation(async (url: string) => {
      if (String(url).includes('phase=title')) {
        return jsonResponse({
          results: [{
            path: '/tmp/agents/hana/sessions/title-hit.jsonl',
            title: '聊天记录搜索',
            firstMessage: 'hello',
            modified: '2026-05-22T08:00:00.000Z',
            messageCount: 2,
            agentId: 'hana',
            agentName: 'Hana',
            cwd: '/tmp/workspace-b',
            matchKind: 'title',
            snippet: '',
          }],
        });
      }
      if (String(url).includes('phase=content')) {
        return jsonResponse({
          results: [{
            path: '/tmp/agents/hana/sessions/content-hit.jsonl',
            title: '排查记录',
            firstMessage: 'hello',
            modified: '2026-05-22T07:00:00.000Z',
            messageCount: 4,
            agentId: 'hana',
            agentName: 'Hana',
            cwd: '/tmp/workspace-b',
            matchKind: 'content',
            snippet: '这里记录了和其他 Agent 的聊天记录排查。',
          }],
        });
      }
      return jsonResponse({});
    });

    render(<ChatSearchOverlay />);
    fireEvent.change(screen.getByPlaceholderText('sidebar.searchPlaceholder'), {
      target: { value: '聊天记录' },
    });

    expect(await screen.findByText('聊天记录搜索')).toBeInTheDocument();
    expect(await screen.findByText(/和其他 Agent 的聊天记录/)).toBeInTheDocument();
    expect(screen.getByText('sidebar.searchTitleMatches')).toBeInTheDocument();
    expect(screen.getByText('sidebar.searchContentMatches')).toBeInTheDocument();

    const searchCalls = lingxiFetchMock.mock.calls
      .map(([url]) => String(url))
      .filter(url => url.startsWith('/api/sessions/search'));
    expect(searchCalls[0]).toContain('phase=title');
    expect(searchCalls[1]).toContain('phase=content');
  });

  it('routes title hits through switchSession and content hits through locateSearchHit', async () => {
    lingxiFetchMock.mockImplementation(async (url: string) => {
      if (String(url).includes('phase=title')) {
        return jsonResponse({
          results: [{
            path: '/tmp/agents/hana/sessions/title-hit.jsonl',
            title: 'Title hit',
            firstMessage: 'hello',
            modified: '2026-05-22T08:00:00.000Z',
            messageCount: 2,
            agentId: 'hana',
            agentName: 'Hana',
            cwd: '/tmp/workspace-b',
            matchKind: 'title',
            snippet: '',
          }],
        });
      }
      if (String(url).includes('phase=content')) {
        return jsonResponse({
          results: [{
            path: '/tmp/agents/hana/sessions/content-hit.jsonl',
            title: 'Content hit',
            firstMessage: 'hello',
            modified: '2026-05-22T07:00:00.000Z',
            messageCount: 4,
            agentId: 'hana',
            agentName: 'Hana',
            cwd: '/tmp/workspace-b',
            matchKind: 'content',
            snippet: '正文片段',
          }],
        });
      }
      return jsonResponse({});
    });

    render(<ChatSearchOverlay />);
    fireEvent.change(screen.getByPlaceholderText('sidebar.searchPlaceholder'), {
      target: { value: '排查' },
    });

    const titleButton = (await screen.findByText('Title hit')).closest('button')!;
    fireEvent.click(titleButton);
    expect(switchSessionMock).toHaveBeenCalledWith('/tmp/agents/hana/sessions/title-hit.jsonl');
    // 点击后界面关闭
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });

    // 重新打开再点内容命中
    act(() => {
      useStore.setState({ chatSearchOpen: true } as never);
    });
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText('sidebar.searchPlaceholder'), {
      target: { value: '排查' },
    });
    const contentButton = (await screen.findByText('Content hit')).closest('button')!;
    fireEvent.click(contentButton);
    expect(locateSearchHitMock).toHaveBeenCalledWith(
      '/tmp/agents/hana/sessions/content-hit.jsonl',
      '排查',
    );
  });

  it('closes on Escape, backdrop click, and the close button', async () => {
    render(<ChatSearchOverlay />);

    fireEvent.keyDown(screen.getByPlaceholderText('sidebar.searchPlaceholder'), { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());

    act(() => {
      useStore.setState({ chatSearchOpen: true } as never);
    });
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    const backdrop = document.querySelector('[data-chat-search-overlay]')?.parentElement;
    expect(backdrop).toBeTruthy();
    fireEvent.mouseDown(backdrop!);
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());

    act(() => {
      useStore.setState({ chatSearchOpen: true } as never);
    });
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'common.close' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  });

  it('shows the no-results state once both phases finish empty', async () => {
    render(<ChatSearchOverlay />);
    fireEvent.change(screen.getByPlaceholderText('sidebar.searchPlaceholder'), {
      target: { value: '不存在' },
    });

    await waitFor(() => {
      expect(screen.getByText('sidebar.searchNoResults')).toBeInTheDocument();
    });
  });
});
