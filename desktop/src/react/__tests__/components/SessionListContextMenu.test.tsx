/**
 * @vitest-environment jsdom
 */
import { act, cleanup, createEvent, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';

const lingxiFetchMock = vi.fn();
const switchSessionMock = vi.fn();
const archiveSessionMock = vi.fn();
const renameSessionMock = vi.fn();
const pinSessionMock = vi.fn();
const createNewSessionMock = vi.fn();
const reorderPinnedSessionsMock = vi.fn();
const openBrowserViewerMock = vi.fn();

const localServerConnection = {
  connectionId: 'local',
  kind: 'local' as const,
  serverId: 'local',
  studioId: 'local',
  label: 'Local Hana',
  baseUrl: 'http://127.0.0.1:3210',
  wsUrl: 'ws://127.0.0.1:3210',
  token: 'test-token',
  authState: 'paired' as const,
  trustState: 'local' as const,
  credentialKind: 'loopback_token' as const,
  capabilities: ['chat', 'resources', 'files', 'tools'],
};

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (...args: unknown[]) => lingxiFetchMock(...args),
  lingxiUrl: (p: string) => p,
}));

vi.mock('../../stores/session-actions', () => ({
  switchSession: (...args: unknown[]) => switchSessionMock(...args),
  archiveSession: (...args: unknown[]) => archiveSessionMock(...args),
  renameSession: (...args: unknown[]) => renameSessionMock(...args),
  pinSession: (...args: unknown[]) => pinSessionMock(...args),
  createNewSession: (...args: unknown[]) => createNewSessionMock(...args),
  reorderPinnedSessions: (...args: unknown[]) => reorderPinnedSessionsMock(...args),
}));

vi.mock('../../hooks/use-i18n', () => ({
  useI18n: () => ({
    t: (key: string) => key === 'session.summary.open' ? '摘要' : key,
  }),
}));

import { SessionList } from '../../components/SessionList';
import { useStore } from '../../stores';

function jsonResponse(data: unknown) {
  return {
    json: async () => data,
  };
}

function seedSessions() {
  useStore.setState({
    sessions: [
      {
        path: '/tmp/agents/hana/sessions/with-summary.jsonl',
        sessionId: 'sess_with_summary',
        title: 'Has summary',
        firstMessage: 'hello',
        modified: '2026-04-29T08:00:00.000Z',
        messageCount: 2,
        agentId: 'hana',
        agentName: 'Hana',
        cwd: '/tmp/project',
        pinnedAt: null,
        hasSummary: true,
      },
      {
        path: '/tmp/agents/hana/sessions/no-summary.jsonl',
        title: 'No summary',
        firstMessage: 'hello',
        modified: '2026-04-29T07:00:00.000Z',
        messageCount: 1,
        agentId: 'hana',
        agentName: 'Hana',
        cwd: '/tmp/project',
        pinnedAt: null,
        hasSummary: false,
      },
    ],
    currentSessionPath: null,
    pendingSessionSwitchPath: null,
    pendingNewSession: false,
    agents: [],
    streamingSessions: [],
    unreadOutputSessionPaths: [],
    browserBySession: {},
    locale: 'zh',
    deskWorkspaceMountId: null,
    deskBasePath: '/tmp/project',
    deskWorkspaceLabel: null,
    selectedFolder: null,
    selectedWorkspaceMountId: null,
    activeServerConnectionId: localServerConnection.connectionId,
    activeServerConnection: localServerConnection,
  });
}

function makeSessionsToday() {
  useStore.setState({
    sessions: useStore.getState().sessions.map((session) => ({
      ...session,
      modified: new Date().toISOString(),
    })),
  });
}

function sessionButton(title: string) {
  const button = screen.getByText(title).closest('button');
  if (!button) throw new Error(`Missing session button: ${title}`);
  return button;
}

function dragData() {
  const data = new Map<string, string>();
  return {
    dropEffect: '',
    effectAllowed: '',
    setData: vi.fn((type: string, value: string) => data.set(type, value)),
    getData: vi.fn((type: string) => data.get(type) || ''),
    clearData: vi.fn(() => data.clear()),
  };
}


describe('SessionList context menu', () => {
  beforeEach(() => {
    window.localStorage.removeItem('hana-session-sidebar-view-mode');
    window.localStorage.removeItem('hana-sidebar-ui-prefs');
    useStore.getState().applySidebarUiPrefs({});
    useStore.setState({ sidebarUiPrefsLoaded: false });
    globalThis.t = ((key: string) => {
      if (key === 'yuan.types') return {};
      return key;
    }) as typeof globalThis.t;
    lingxiFetchMock.mockReset();
    lingxiFetchMock.mockImplementation(async (url: string) => {
      if (url === '/api/browser/session-states') return jsonResponse({});
      if (url === '/api/browser/sessions') return jsonResponse({});
      if (url.startsWith('/api/sessions/summary')) {
        return jsonResponse({
          hasSummary: true,
          summary: '### 重要事实\n- 用户在做记忆系统。\n\n### 事情经过\n- 10:00 用户讨论 session 摘要。',
          createdAt: '2026-04-29T07:00:00.000Z',
          updatedAt: '2026-04-29T08:00:00.000Z',
        });
      }
      return jsonResponse({});
    });
    switchSessionMock.mockReset();
    archiveSessionMock.mockReset();
    renameSessionMock.mockReset();
    pinSessionMock.mockReset();
    createNewSessionMock.mockReset();
    reorderPinnedSessionsMock.mockReset();
    openBrowserViewerMock.mockReset();
    Object.defineProperty(window, 'platform', {
      configurable: true,
      value: { openBrowserViewer: openBrowserViewerMock },
    });
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: vi.fn(async () => undefined) },
    });
    seedSessions();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it('keeps summaryless session rows readable and disables only the summary menu item', () => {
    render(<SessionList />);

    expect(sessionButton('No summary').className).not.toContain('sessionItemSummaryEmpty');

    fireEvent.contextMenu(sessionButton('No summary'), { clientX: 24, clientY: 32 });
    const summaryItem = screen.getByText('摘要').closest('.context-menu-item');
    expect(summaryItem).toHaveClass('disabled');

    fireEvent.click(screen.getByText('摘要'));
    expect(screen.queryByTestId('session-summary-card')).not.toBeInTheDocument();
    expect(lingxiFetchMock).not.toHaveBeenCalledWith(
      '/api/sessions/summary?path=%2Ftmp%2Fagents%2Fhana%2Fsessions%2Fno-summary.jsonl',
    );
  });

  it('keeps the right-click menu as a shared narrow menu and opens summary as a click-through preview card', async () => {
    render(<SessionList />);

    fireEvent.contextMenu(sessionButton('Has summary'), { clientX: 24, clientY: 32 });

    const menu = document.querySelector('.context-menu');
    expect(menu).toBeInTheDocument();
    expect(menu).toHaveClass('context-menu');
    expect(menu?.className).toBe('context-menu');
    expect(screen.getByText('摘要')).toBeInTheDocument();
    expect(menu?.querySelector('.context-menu-divider')).toBeNull();
    expect(screen.queryByTestId('session-summary-card')).not.toBeInTheDocument();
    expect(lingxiFetchMock).not.toHaveBeenCalledWith(
      '/api/sessions/summary?path=%2Ftmp%2Fagents%2Fhana%2Fsessions%2Fwith-summary.jsonl',
    );

    fireEvent.click(screen.getByText('摘要'));

    expect(await screen.findByTestId('session-summary-card')).toHaveAttribute('data-scrollable', 'true');
    expect(await screen.findByText(/用户在做记忆系统/)).toBeInTheDocument();
    expect(lingxiFetchMock).toHaveBeenCalledWith(
      '/api/sessions/summary?path=%2Ftmp%2Fagents%2Fhana%2Fsessions%2Fwith-summary.jsonl',
    );
  });

  it('routes context menu actions through the existing session operations', async () => {
    render(<SessionList />);

    fireEvent.contextMenu(sessionButton('Has summary'), { clientX: 24, clientY: 32 });
    fireEvent.click(await screen.findByText('session.pin'));
    expect(pinSessionMock).toHaveBeenCalledWith('/tmp/agents/hana/sessions/with-summary.jsonl', true);

    fireEvent.contextMenu(sessionButton('No summary'), { clientX: 24, clientY: 32 });
    fireEvent.click(await screen.findByText('session.rename'));
    const input = screen.getByDisplayValue('No summary');
    fireEvent.change(input, { target: { value: 'Renamed summaryless session' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(renameSessionMock).toHaveBeenCalledWith(
      '/tmp/agents/hana/sessions/no-summary.jsonl',
      'Renamed summaryless session',
    );

    fireEvent.contextMenu(sessionButton('Has summary'), { clientX: 24, clientY: 32 });
    fireEvent.click(await screen.findByText('session.archive'));
    expect(archiveSessionMock).toHaveBeenCalledWith('/tmp/agents/hana/sessions/with-summary.jsonl');
  });

  it('copies only the stable Session ID and disables the action when it is unavailable', async () => {
    render(<SessionList />);

    fireEvent.contextMenu(sessionButton('Has summary'), { clientX: 24, clientY: 32 });
    fireEvent.click(screen.getByText('session.copyId'));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('sess_with_summary');

    fireEvent.contextMenu(sessionButton('No summary'), { clientX: 24, clientY: 32 });
    expect(screen.getByText('session.copyId').closest('.context-menu-item')).toHaveClass('disabled');
  });

  it('allows deleted-agent sessions to unpin and archive without exposing rename or pin', async () => {
    useStore.setState({
      sessions: [{
        path: '/tmp/agents/deleted/sessions/pinned.jsonl',
        title: 'Deleted pinned',
        firstMessage: 'old',
        modified: '2026-04-29T08:00:00.000Z',
        messageCount: 2,
        agentId: 'deleted',
        agentName: 'Deleted',
        cwd: '/tmp/project',
        pinnedAt: '2026-04-29T08:10:00.000Z',
        hasSummary: false,
        agentDeleted: true,
      }],
      currentSessionPath: null,
      pendingSessionSwitchPath: null,
      pendingNewSession: false,
      agents: [],
      streamingSessions: [],
      unreadOutputSessionPaths: [],
      browserBySession: {},
      locale: 'zh',
      deskWorkspaceMountId: null,
      deskBasePath: '/tmp/project',
      selectedFolder: null,
      selectedWorkspaceMountId: null,
    });

    render(<SessionList />);

    fireEvent.contextMenu(sessionButton('Deleted pinned'), { clientX: 24, clientY: 32 });
    expect(screen.queryByText('session.rename')).not.toBeInTheDocument();
    expect(screen.queryByText('session.pin')).not.toBeInTheDocument();
    fireEvent.click(await screen.findByText('session.unpin'));
    expect(pinSessionMock).toHaveBeenCalledWith('/tmp/agents/deleted/sessions/pinned.jsonl', false);

    fireEvent.contextMenu(sessionButton('Deleted pinned'), { clientX: 24, clientY: 32 });
    fireEvent.click(await screen.findByText('session.archive'));
    expect(archiveSessionMock).toHaveBeenCalledWith('/tmp/agents/deleted/sessions/pinned.jsonl');
  });

  it('opens the session browser from a left click on the sidebar badge', async () => {
    const browserStates = {
      '/tmp/agents/hana/sessions/with-summary.jsonl': {
        url: 'https://example.com',
        running: false,
        resumable: true,
        unavailableReason: null,
      },
    };
    lingxiFetchMock.mockImplementation(async (url: string) => {
      if (url === '/api/browser/session-states') return jsonResponse(browserStates);
      if (url === '/api/browser/open-session') return jsonResponse({ ok: true });
      return jsonResponse({});
    });

    render(<SessionList />);

    fireEvent.click(await screen.findByRole('button', { name: 'browser.open' }));

    await waitFor(() => {
      expect(lingxiFetchMock).toHaveBeenCalledWith('/api/browser/open-session', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ sessionPath: '/tmp/agents/hana/sessions/with-summary.jsonl' }),
      }));
      expect(openBrowserViewerMock).toHaveBeenCalledWith({
        sessionPath: '/tmp/agents/hana/sessions/with-summary.jsonl',
      });
    });
    expect(lingxiFetchMock).not.toHaveBeenCalledWith('/api/browser/close-session', expect.anything());
    expect(switchSessionMock).not.toHaveBeenCalled();
    expect(await screen.findByRole('button', { name: 'browser.open' })).toBeInTheDocument();
  });

  it('closes the session browser from the badge context menu', async () => {
    const browserStates = {
      '/tmp/agents/hana/sessions/with-summary.jsonl': {
        url: 'https://example.com',
        running: true,
        resumable: true,
        unavailableReason: null,
      },
    };
    let closed = false;
    lingxiFetchMock.mockImplementation(async (url: string) => {
      if (url === '/api/browser/session-states') return jsonResponse(closed ? {} : browserStates);
      if (url === '/api/browser/close-session') {
        closed = true;
        return jsonResponse({ ok: true, sessions: {} });
      }
      return jsonResponse({});
    });

    render(<SessionList />);

    fireEvent.contextMenu(await screen.findByRole('button', { name: 'browser.open' }), {
      clientX: 40,
      clientY: 60,
    });
    fireEvent.click(await screen.findByText('browser.closeForSession'));

    await waitFor(() => {
      expect(lingxiFetchMock).toHaveBeenCalledWith('/api/browser/close-session', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ sessionPath: '/tmp/agents/hana/sessions/with-summary.jsonl' }),
      }));
    });
    expect(switchSessionMock).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'browser.open' })).not.toBeInTheDocument();
    });
  });

  it('applies the persisted single-line row mode from the store to regular session rows', () => {
    act(() => {
      useStore.getState().applySidebarUiPrefs({
        sidebarUi: {
          projectView: { collapsedProjectIds: [], collapsedFolderIds: [], showAllProjectIds: [] },
          sessionList: { rowMode: 'single-line' },
        },
      });
    });

    render(<SessionList />);

    const row = sessionButton('Has summary');
    expect(row).toHaveAttribute('data-row-mode', 'single-line');
    expect(row.querySelector('[data-session-actions]')).toBeInTheDocument();
    expect(row).toHaveAttribute('title', expect.stringContaining('Hana'));
  });

  it('never fetches sidebar UI preferences itself', async () => {
    render(<SessionList />);

    await waitFor(() => {
      expect(lingxiFetchMock).toHaveBeenCalledWith('/api/browser/session-states');
    });
    expect(lingxiFetchMock).not.toHaveBeenCalledWith('/api/preferences/sidebar-ui');
  });

  it('follows the row mode when the store picks up new preferences after mount', async () => {
    render(<SessionList />);
    expect(sessionButton('Has summary')).toHaveAttribute('data-row-mode', 'two-line');

    act(() => {
      useStore.getState().applySidebarUiPrefs({
        sidebarUi: {
          projectView: { collapsedProjectIds: [], collapsedFolderIds: [], showAllProjectIds: [] },
          sessionList: { rowMode: 'single-line' },
        },
      });
    });

    await waitFor(() => {
      expect(sessionButton('Has summary')).toHaveAttribute('data-row-mode', 'single-line');
    });
  });

  it('keeps single-line rows on the very first frame after a remount, without any request', () => {
    act(() => {
      useStore.getState().applySidebarUiPrefs({
        sidebarUi: {
          projectView: { collapsedProjectIds: [], collapsedFolderIds: [], showAllProjectIds: [] },
          sessionList: { rowMode: 'single-line' },
        },
      });
    });

    const first = render(<SessionList />);
    expect(sessionButton('Has summary')).toHaveAttribute('data-row-mode', 'single-line');
    first.unmount();

    render(<SessionList />);
    expect(sessionButton('Has summary')).toHaveAttribute('data-row-mode', 'single-line');
    expect(lingxiFetchMock).not.toHaveBeenCalledWith('/api/preferences/sidebar-ui');
  });

  it('uses the session meta font size for the summary body', () => {
    const css = fs.readFileSync(
      path.join(__dirname, '../../components/SessionList.module.css'),
      'utf-8',
    );

    expect(css).toMatch(/\.sessionSummaryBody\s*\{[\s\S]*font-size:\s*var\(--fs-hint\)/);
    expect(css).not.toMatch(/\.sessionContextMenu/);
    expect(css).not.toMatch(/sessionItemSummaryEmpty/);
  });

  it('uses one fine-hover policy for row hover controls', () => {
    const css = fs.readFileSync(
      path.join(__dirname, '../../components/SessionList.module.css'),
      'utf-8',
    );

    expect(css).not.toMatch(/@media\s*\(hover:\s*hover\)\s*and\s*\(pointer:\s*fine\)/);
    expect(css).toMatch(/@media\s*\(any-hover:\s*hover\)\s*and\s*\(any-pointer:\s*fine\)\s*\{[\s\S]*\.sessionItem:hover\s*\{/);
    expect(css).toMatch(/@media\s*\(any-hover:\s*hover\)\s*and\s*\(any-pointer:\s*fine\)\s*\{[\s\S]*\.sessionItem:not\(\.sessionItemSingleLine\):hover \.sessionArchiveBtn/);
    expect(css).toMatch(/@media\s*\(any-hover:\s*hover\)\s*and\s*\(any-pointer:\s*fine\)\s*\{[\s\S]*\.sessionItemSingleLine:hover \.sessionItemActions\s*\{[\s\S]*width:\s*calc\(40px \+ var\(--space-4\)\)/);
  });

  it('drops the removed sidebar search box styles along with the search UI', () => {
    const css = fs.readFileSync(
      path.join(__dirname, '../../components/SessionList.module.css'),
      'utf-8',
    );

    expect(css).not.toContain('sessionSearchBox');
    expect(css).not.toContain('sessionSearchInput');
    expect(css).not.toContain('projectRow');
    expect(css).not.toContain('sectionIconButton');
  });

  it('keeps row action controls hover-only and leaves active rows from reserving empty action space', () => {
    const css = fs.readFileSync(
      path.join(__dirname, '../../components/SessionList.module.css'),
      'utf-8',
    );

    expect(css).toMatch(/\.sessionItem:not\(\.sessionItemSingleLine\):hover \.sessionPinBtn/);
    expect(css).toMatch(/\.sessionItem:not\(\.sessionItemSingleLine\):hover \.sessionArchiveBtn/);
    expect(css).toMatch(/\.sessionItem:not\(\.sessionItemSingleLine\):hover \.sessionItemMeta\s*\{[\s\S]*padding-right:\s*52px/);
    expect(css).toMatch(/\.sessionItem:not\(\.sessionItemSingleLine\) \.sessionItemActions\s*\{[\s\S]*position:\s*absolute/);
    expect(css).toMatch(/\.sessionItemSingleLine \.sessionItemActions\s*\{[\s\S]*width:\s*0/);
    expect(css).not.toMatch(/\.sessionItemActive \.sessionPinBtn/);
    expect(css).not.toMatch(/\.sessionItemActive \.sessionArchiveBtn/);
    expect(css).not.toMatch(/\.sessionItemActive \.sessionItemMeta/);
    expect(css).not.toMatch(/sessionRenameBtn/);
  });

  it('keeps rename in the context menu without rendering an inline rename button', async () => {
    render(<SessionList />);

    expect(screen.queryByTitle('session.rename')).not.toBeInTheDocument();

    fireEvent.contextMenu(sessionButton('No summary'), { clientX: 24, clientY: 32 });
    fireEvent.click(await screen.findByText('session.rename'));

    expect(screen.getByDisplayValue('No summary')).toBeInTheDocument();
  });

  it('renders unread output and running status as row-level status signals', async () => {
    useStore.setState({
      currentSessionPath: '/tmp/agents/hana/sessions/no-summary.jsonl',
      streamingSessions: ['/tmp/agents/hana/sessions/with-summary.jsonl'],
      unreadOutputSessionPaths: ['/tmp/agents/hana/sessions/with-summary.jsonl'],
    } as never);

    render(<SessionList />);

    const row = sessionButton('Has summary');
    expect(row).toHaveAttribute('data-unread-output', 'true');
    const dot = row.querySelector('[data-session-status-dot]');
    expect(dot).toBeInTheDocument();
    expect(dot).toHaveAttribute('data-state', 'running');
  });

  it('marks the pending switch row immediately without changing the committed session path', () => {
    useStore.setState({
      currentSessionPath: '/tmp/agents/hana/sessions/no-summary.jsonl',
      pendingSessionSwitchPath: '/tmp/agents/hana/sessions/with-summary.jsonl',
      streamingSessions: [],
      unreadOutputSessionPaths: [],
    } as never);

    render(<SessionList />);

    const pendingRow = sessionButton('Has summary');
    expect(pendingRow).toHaveAttribute('data-switch-pending', 'true');

    const currentRow = sessionButton('No summary');
    expect(currentRow).toHaveAttribute('data-switch-pending', 'false');
  });

  // 切换会话是本地操作，通常几十毫秒就完成。此前它会借用「正在输出」的状态点，
  // 结果每次点列表都闪一下，既是视觉噪音，也把「这个会话正在跑」的语义冲淡了。
  it('shows no status dot while a session switch is loading', () => {
    useStore.setState({
      currentSessionPath: '/tmp/agents/hana/sessions/no-summary.jsonl',
      pendingSessionSwitchPath: '/tmp/agents/hana/sessions/with-summary.jsonl',
      streamingSessions: [],
      unreadOutputSessionPaths: [],
    } as never);

    render(<SessionList />);

    const pendingRow = sessionButton('Has summary');
    expect(pendingRow.querySelector('[data-session-status-dot]')).not.toBeInTheDocument();
  });

  it('keeps the running dot on a session that is both switching and streaming', () => {
    useStore.setState({
      currentSessionPath: '/tmp/agents/hana/sessions/no-summary.jsonl',
      pendingSessionSwitchPath: '/tmp/agents/hana/sessions/with-summary.jsonl',
      streamingSessions: ['/tmp/agents/hana/sessions/with-summary.jsonl'],
      unreadOutputSessionPaths: [],
    } as never);

    render(<SessionList />);

    const dot = sessionButton('Has summary').querySelector('[data-session-status-dot]');
    expect(dot).toBeInTheDocument();
    expect(dot).toHaveAttribute('data-state', 'running');
  });

  it('keeps the status dot after a background session finishes until the user opens it', () => {
    useStore.setState({
      currentSessionPath: '/tmp/agents/hana/sessions/no-summary.jsonl',
      streamingSessions: [],
      unreadOutputSessionPaths: ['/tmp/agents/hana/sessions/with-summary.jsonl'],
    } as never);

    render(<SessionList />);

    const row = sessionButton('Has summary');
    const dot = row.querySelector('[data-session-status-dot]');
    expect(dot).toBeInTheDocument();
    expect(dot).toHaveAttribute('data-state', 'unread');
  });

  it('does not reference removed session row status affordances', () => {
    const source = fs.readFileSync(
      path.join(__dirname, '../../components/SessionList.tsx'),
      'utf-8',
    );

    expect(source).not.toContain('sessionItemHeaderWithStatus');
    expect(source).not.toContain('sessionStreamingRing');
    const css = fs.readFileSync(
      path.join(__dirname, '../../components/SessionList.module.css'),
      'utf-8',
    );
    expect(css).not.toContain('sessionItemUnreadOutput');
    expect(css).not.toContain('sessionStreamingRing');
  });

  it('scopes the session list to the active workspace instead of showing every session', () => {
    useStore.setState({
      sessions: [
        {
          path: '/tmp/agents/hana/sessions/in-scope.jsonl',
          title: 'In scope',
          firstMessage: 'hello',
          modified: new Date().toISOString(),
          messageCount: 1,
          agentId: 'hana',
          agentName: 'Hana',
          cwd: '/tmp/project',
          pinnedAt: null,
          hasSummary: false,
        },
        {
          path: '/tmp/agents/hana/sessions/other-dir.jsonl',
          title: 'Other dir',
          firstMessage: 'hello',
          modified: new Date().toISOString(),
          messageCount: 1,
          agentId: 'hana',
          agentName: 'Hana',
          cwd: '/tmp/other-project',
          pinnedAt: null,
          hasSummary: false,
        },
        {
          path: '/tmp/agents/hana/sessions/mount.jsonl',
          title: 'Mount session',
          firstMessage: 'hello',
          modified: new Date().toISOString(),
          messageCount: 1,
          agentId: 'hana',
          agentName: 'Hana',
          cwd: null,
          workspaceMountId: 'mount-abc',
          pinnedAt: null,
          hasSummary: false,
        },
        {
          path: '/tmp/agents/hana/sessions/no-identity.jsonl',
          title: 'No identity',
          firstMessage: 'hello',
          modified: new Date().toISOString(),
          messageCount: 1,
          agentId: 'hana',
          agentName: 'Hana',
          cwd: null,
          pinnedAt: null,
          hasSummary: false,
        },
      ],
    } as never);

    render(<SessionList />);

    // 本地目录作用域（deskBasePath=/tmp/project）：只有同 cwd 的会话可见，
    // 其他目录 / mount / 无身份会话在数据层就被排除。
    expect(sessionButton('In scope')).toBeInTheDocument();
    expect(screen.queryByText('Other dir')).not.toBeInTheDocument();
    expect(screen.queryByText('Mount session')).not.toBeInTheDocument();
    expect(screen.queryByText('No identity')).not.toBeInTheDocument();
  });

  it('re-filters the list when the workspace switches', () => {
    render(<SessionList />);
    expect(sessionButton('Has summary')).toBeInTheDocument();

    // 切到另一个工作台：列表跟随 desk 状态响应式重过滤。
    act(() => {
      useStore.setState({ deskBasePath: '/tmp/elsewhere' });
    });
    expect(screen.queryByText('Has summary')).not.toBeInTheDocument();
    expect(screen.getByText('sidebar.empty')).toBeInTheDocument();

    // 切回 mount 工作台：只有该 mount 的会话可见。
    act(() => {
      useStore.setState({ deskBasePath: '', deskWorkspaceMountId: 'mount-abc' });
    });
    expect(screen.queryByText('Has summary')).not.toBeInTheDocument();

    // pending 新会话：作用域取 selectedFolder。
    act(() => {
      useStore.setState({ deskWorkspaceMountId: null, selectedFolder: '/tmp/project' });
    });
    expect(sessionButton('Has summary')).toBeInTheDocument();
  });

  it('removes the resident search box and the project/time view navigation', () => {
    render(<SessionList />);

    expect(screen.queryByPlaceholderText('sidebar.searchPlaceholder')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'sidebar.view.sort' })).not.toBeInTheDocument();
    expect(screen.queryByText('sidebar.projects.title')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'sidebar.projects.create' })).not.toBeInTheDocument();

    const source = fs.readFileSync(
      path.join(__dirname, '../../components/SessionList.tsx'),
      'utf-8',
    );
    expect(source).not.toContain('SessionSearchBox');
    expect(source).not.toContain('ProjectSessionView');
    expect(source).not.toContain('viewMode');
    expect(source).not.toContain("localStorage");
  });

  describe('pinned strip reordering', () => {
    function seedPinnedSessions(options: { withSessionIds?: boolean } = {}) {
      const withSessionIds = options.withSessionIds !== false;
      useStore.setState({
        sessions: [
          {
            path: '/tmp/agents/hana/sessions/pin-a.jsonl',
            sessionId: withSessionIds ? 'sess_pin_a' : null,
            title: 'Pin A',
            firstMessage: 'a',
            modified: '2026-04-29T08:00:00.000Z',
            messageCount: 1,
            agentId: 'hana',
            agentName: 'Hana',
            cwd: '/tmp/project',
            pinnedAt: '2026-04-28T07:00:00.000Z',
            pinOrder: 1024,
          },
          {
            path: '/tmp/agents/hana/sessions/pin-b.jsonl',
            sessionId: withSessionIds ? 'sess_pin_b' : null,
            title: 'Pin B',
            firstMessage: 'b',
            modified: '2026-04-29T07:00:00.000Z',
            messageCount: 1,
            agentId: 'hana',
            agentName: 'Hana',
            cwd: '/tmp/project',
            pinnedAt: '2026-04-28T07:00:00.000Z',
            pinOrder: 2048,
          },
          {
            path: '/tmp/agents/hana/sessions/pin-c.jsonl',
            sessionId: withSessionIds ? 'sess_pin_c' : null,
            title: 'Pin C',
            firstMessage: 'c',
            modified: '2026-04-29T06:00:00.000Z',
            messageCount: 1,
            agentId: 'hana',
            agentName: 'Hana',
            cwd: '/tmp/project',
            pinnedAt: '2026-04-28T07:00:00.000Z',
            pinOrder: 3072,
          },
        ],
      } as never);
    }

    function pinnedRow(title: string) {
      const row = sessionButton(title).closest('[data-pinned-session-path]');
      if (!row) throw new Error(`Missing pinned row: ${title}`);
      return row as HTMLElement;
    }

    function stubRowGeometry(row: HTMLElement) {
      row.getBoundingClientRect = () => ({
        top: 0, bottom: 40, left: 0, right: 100, width: 100, height: 40, x: 0, y: 0,
        toJSON: () => ({}),
      }) as DOMRect;
    }

    // jsdom has no DragEvent, so fireEvent cannot carry pointer coordinates on a
    // drag event; define them on the event object the way a browser would.
    function fireDragAt(
      type: 'dragOver' | 'drop',
      row: HTMLElement,
      dataTransfer: ReturnType<typeof dragData>,
      clientY: number,
    ) {
      const event = createEvent[type](row, { dataTransfer });
      Object.defineProperty(event, 'clientY', { value: clientY });
      fireEvent(row, event);
    }

    it('submits the full pinned order when a row is dropped above another row', async () => {
      seedPinnedSessions();
      render(<SessionList />);

      const dataTransfer = dragData();
      fireEvent.dragStart(sessionButton('Pin C'), { dataTransfer });
      const target = pinnedRow('Pin A');
      stubRowGeometry(target);
      fireDragAt('dragOver', target, dataTransfer, 5);
      fireDragAt('drop', target, dataTransfer, 5);

      await waitFor(() => {
        expect(reorderPinnedSessionsMock).toHaveBeenCalledWith([
          'sess_pin_c',
          'sess_pin_a',
          'sess_pin_b',
        ]);
      });
    });

    it('submits the order with the dragged row below the target when dropped on its lower half', async () => {
      seedPinnedSessions();
      render(<SessionList />);

      const dataTransfer = dragData();
      fireEvent.dragStart(sessionButton('Pin A'), { dataTransfer });
      const target = pinnedRow('Pin B');
      stubRowGeometry(target);
      fireDragAt('dragOver', target, dataTransfer, 35);
      fireDragAt('drop', target, dataTransfer, 35);

      await waitFor(() => {
        expect(reorderPinnedSessionsMock).toHaveBeenCalledWith([
          'sess_pin_b',
          'sess_pin_a',
          'sess_pin_c',
        ]);
      });
    });

    it('disables pinned reordering entirely when any pinned row has no session id', () => {
      seedPinnedSessions({ withSessionIds: false });
      render(<SessionList />);

      expect(sessionButton('Pin A')).not.toHaveAttribute('draggable', 'true');

      const dataTransfer = dragData();
      fireEvent.dragStart(sessionButton('Pin C'), { dataTransfer });
      fireDragAt('drop', pinnedRow('Pin A'), dataTransfer, 5);

      expect(reorderPinnedSessionsMock).not.toHaveBeenCalled();
    });
  });

  it('keeps the pinned heading font unified with date headings', () => {
    const css = fs.readFileSync(
      path.join(__dirname, '../../components/SessionList.module.css'),
      'utf-8',
    );

    const baseTitleRule = css.match(/\.sessionSectionTitle\s*\{[^}]*\}/)?.[0] || '';
    const pinnedTitleRule = css.match(/\.pinnedSection \.sessionSectionTitle\s*\{[^}]*\}/)?.[0] || '';
    expect(baseTitleRule).toContain('font-size: var(--fs-ui)');
    expect(pinnedTitleRule).not.toContain('font-size:');
  });
});
