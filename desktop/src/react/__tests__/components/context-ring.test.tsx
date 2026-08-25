// @vitest-environment jsdom

import { cleanup, render, waitFor } from '@testing-library/react';
import { fireEvent, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ContextRing } from '../../components/input/ContextRing';
import { useStore } from '../../stores';
import { refreshSessionCapabilities } from '../../stores/session-actions';

const { sendMock, getWebSocketMock, lingxiFetchMock } = vi.hoisted(() => ({
  sendMock: vi.fn(),
  getWebSocketMock: vi.fn(),
  lingxiFetchMock: vi.fn(),
}));

vi.mock('../../services/websocket', () => ({
  getWebSocket: getWebSocketMock,
}));

vi.mock('../../stores/session-actions', () => ({
  refreshSessionCapabilities: vi.fn(() => Promise.resolve(true)),
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: lingxiFetchMock,
}));

describe('ContextRing', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    lingxiFetchMock.mockResolvedValue(new Response(JSON.stringify({
      experiments: [{ id: 'session.instant_simple_compaction', value: false }],
    })));
    getWebSocketMock.mockReturnValue({ readyState: WebSocket.OPEN, send: sendMock });
    useStore.setState({
      agentYuan: 'lingxi',
      currentSessionId: 'sess_a',
      currentSessionPath: '/session/a.jsonl',
      contextTokens: null,
      contextWindow: null,
      contextPercent: null,
      contextBySession: {},
      compactingSessions: ['/session/a.jsonl'],
      compactionModeBySession: {},
    } as never);
  });

  afterEach(() => {
    cleanup();
    useStore.setState({
      currentSessionPath: null,
      currentSessionId: null,
      contextTokens: null,
      contextWindow: null,
      contextPercent: null,
      contextBySession: {},
      compactingSessions: [],
      compactionModeBySession: {},
    } as never);
  });

  it('stays visible while the current session is compacting before usage arrives', async () => {
    const { container } = render(<ContextRing />);

    await waitFor(() => {
      const button = container.querySelector('button');
      expect(button).toBeTruthy();
      expect((button as HTMLButtonElement).disabled).toBe(true);
    });
  });

  it('identifies instant simple compaction in the ring tooltip', async () => {
    useStore.setState({
      compactionModeBySession: { sess_a: 'lossy_local' },
    } as never);
    const { container } = render(<ContextRing />);

    fireEvent.mouseEnter(container.querySelector('button') as HTMLButtonElement);

    await waitFor(() => {
      expect(screen.getByText('chat.instantSimpleCompaction')).toBeInTheDocument();
    });
  });

  it('is visible for an active session but never shows the token label', async () => {
    useStore.setState({
      contextBySession: {
        '/session/a.jsonl': { tokens: 12_345, window: 200_000, percent: 6 },
      },
      compactingSessions: [],
    } as never);

    const { container, queryByText } = render(<ContextRing />);

    await waitFor(() => {
      expect(container.querySelector('button')).toBeTruthy();
    });
    expect(queryByText('12k')).toBeNull();
  });

  it('keeps the token label hidden at high usage', async () => {
    useStore.setState({
      contextBySession: {
        '/session/a.jsonl': { tokens: 100_000, window: 200_000, percent: 50 },
      },
      compactingSessions: [],
    } as never);

    const { container, queryByText } = render(<ContextRing />);

    await waitFor(() => {
      expect(container.querySelector('button')).toBeTruthy();
    });
    expect(queryByText('100k')).toBeNull();
  });

  it('opens an action menu instead of compacting immediately', async () => {
    useStore.setState({
      compactingSessions: [],
    } as never);

    const { container } = render(<ContextRing />);
    const button = container.querySelector('button') as HTMLButtonElement;
    fireEvent.click(button);

    expect(screen.getByRole('menu')).toBeInTheDocument();
    expect(screen.getAllByRole('menuitem').map(item => item.textContent)).toEqual([
      'input.compact',
      'input.refreshAndCompact',
      'input.contextDetail',
    ]);
    expect(screen.queryByText('chat.instantSimpleCompaction')).not.toBeInTheDocument();
    expect(sendMock).not.toHaveBeenCalled();
  });

  it('shows and runs instant simple compaction only when its experiment is enabled', async () => {
    lingxiFetchMock.mockResolvedValueOnce(new Response(JSON.stringify({
      experiments: [{ id: 'session.instant_simple_compaction', value: true }],
    })));
    useStore.setState({ compactingSessions: [] } as never);

    const { container } = render(<ContextRing />);
    await waitFor(() => expect(lingxiFetchMock).toHaveBeenCalledWith('/api/experiments'));
    fireEvent.click(container.querySelector('button') as HTMLButtonElement);
    const actions = await screen.findAllByRole('menuitem');
    expect(actions.map(item => item.textContent)).toEqual([
      'input.compact',
      'input.refreshAndCompact',
      'chat.instantSimpleCompaction',
      'input.contextDetail',
    ]);
    fireEvent.click(actions[2]);

    expect(sendMock).toHaveBeenCalledWith(JSON.stringify({
      type: 'compact',
      sessionId: 'sess_a',
      method: 'instant_simple',
    }));
    expect(refreshSessionCapabilities).not.toHaveBeenCalled();
  });

  it('updates the one-shot menu entry when the settings window broadcasts the toggle', async () => {
    useStore.setState({ compactingSessions: [] } as never);
    const { container } = render(<ContextRing />);
    await waitFor(() => expect(lingxiFetchMock).toHaveBeenCalledWith('/api/experiments'));

    window.dispatchEvent(new CustomEvent('hana-settings', {
      detail: {
        type: 'experiment-changed',
        id: 'session.instant_simple_compaction',
        value: true,
      },
    }));
    fireEvent.click(container.querySelector('button') as HTMLButtonElement);

    expect(await screen.findByText('chat.instantSimpleCompaction')).toBeInTheDocument();
  });

  it('runs fresh compact from the update action', async () => {
    useStore.setState({
      compactingSessions: [],
    } as never);

    const { container } = render(<ContextRing />);
    fireEvent.click(container.querySelector('button') as HTMLButtonElement);
    fireEvent.click(screen.getByText('input.refreshAndCompact'));

    expect(refreshSessionCapabilities).toHaveBeenCalledWith('/session/a.jsonl');
    expect(sendMock).not.toHaveBeenCalled();
  });

  it('shows a tooltip for the update action', async () => {
    useStore.setState({
      compactingSessions: [],
    } as never);

    const { container } = render(<ContextRing />);
    fireEvent.click(container.querySelector('button') as HTMLButtonElement);
    fireEvent.mouseEnter(screen.getByText('input.refreshAndCompact'));

    await waitFor(() => {
      expect(screen.getByText('input.refreshAndCompactTooltip')).toBeInTheDocument();
    });
  });

  it('runs ordinary compact from the compact action', async () => {
    useStore.setState({
      compactingSessions: [],
    } as never);

    const { container } = render(<ContextRing />);
    fireEvent.click(container.querySelector('button') as HTMLButtonElement);
    fireEvent.click(screen.getByText('input.compact'));

    expect(sendMock).toHaveBeenCalledWith(JSON.stringify({ type: 'compact', sessionId: 'sess_a' }));
    expect(refreshSessionCapabilities).not.toHaveBeenCalled();
  });

  it('shows an error instead of sending when session identity is unavailable', () => {
    useStore.setState({ currentSessionId: null, compactingSessions: [] } as never);

    const { container } = render(<ContextRing />);
    fireEvent.click(container.querySelector('button') as HTMLButtonElement);
    fireEvent.click(screen.getByText('input.compact'));

    expect(sendMock).not.toHaveBeenCalled();
    expect(useStore.getState().toasts.at(-1)).toMatchObject({ type: 'error' });
  });

  it('shows an error instead of silently dropping while WebSocket is disconnected', () => {
    getWebSocketMock.mockReturnValue({ readyState: WebSocket.CLOSED, send: sendMock });
    useStore.setState({ compactingSessions: [] } as never);

    const { container } = render(<ContextRing />);
    fireEvent.click(container.querySelector('button') as HTMLButtonElement);
    fireEvent.click(screen.getByText('input.compact'));

    expect(sendMock).not.toHaveBeenCalled();
    expect(useStore.getState().toasts.at(-1)).toMatchObject({ type: 'error' });
  });

  it('shows the breakdown detail for the current session from the keyed store', () => {
    const breakdown = {
      system: 400, skills: 100, files: 0, tools: 200, mcp: 40,
      conversation: 800, user: 60, toolResults: 100, other: 300,
      total: 2000, computedAt: 1,
    };
    const otherSessionBreakdown = { ...breakdown, system: 9999, total: 9999 };
    useStore.setState({
      compactingSessions: [],
      contextBySession: {
        '/session/a.jsonl': { tokens: 2000, window: 200_000, percent: 1, breakdown },
        '/session/b.jsonl': { tokens: 9999, window: 200_000, percent: 5, breakdown: otherSessionBreakdown },
      },
    } as never);

    const { container } = render(<ContextRing />);
    fireEvent.click(container.querySelector('button') as HTMLButtonElement);
    fireEvent.click(screen.getByText('input.contextDetail'));

    expect(screen.getByText('input.contextDetailUsed')).toBeInTheDocument();
    expect(screen.getByText('input.contextDetailWindowTotal')).toBeInTheDocument();
    expect(screen.getByText('input.contextDetailRemaining')).toBeInTheDocument();
    expect(screen.getByText('input.contextCategory.system')).toBeInTheDocument();
    expect(screen.getByText('input.contextCategory.conversation')).toBeInTheDocument();
    expect(screen.getByText('input.contextCategory.user')).toBeInTheDocument();
    expect(screen.getByText('input.contextCategory.tools')).toBeInTheDocument();
    expect(screen.getByText('input.contextCategory.toolResults')).toBeInTheDocument();
    expect(screen.getByText('input.contextCategory.mcp')).toBeInTheDocument();
    expect(screen.getByText('input.contextCategory.skills')).toBeInTheDocument();
    expect(screen.getByText('input.contextCategory.other')).toBeInTheDocument();
    // 0 值分类不显示,不得制造不存在的上下文项。
    expect(screen.queryByText('input.contextCategory.files')).not.toBeInTheDocument();
    // 详情属于当前 session(session a),不串 session b 的明细。
    expect(screen.getByText('2,000')).toBeInTheDocument();
    expect(screen.queryByText('9,999')).not.toBeInTheDocument();
    // 原菜单项被详情视图替代,Ring 本身不受影响。
    expect(screen.queryByText('input.compact')).not.toBeInTheDocument();
  });

  it('returns from the detail view to the action menu', () => {
    useStore.setState({
      compactingSessions: [],
      contextBySession: {
        '/session/a.jsonl': { tokens: 100, window: 200_000, percent: 1, breakdown: null },
      },
    } as never);

    const { container } = render(<ContextRing />);
    fireEvent.click(container.querySelector('button') as HTMLButtonElement);
    fireEvent.click(screen.getByText('input.contextDetail'));
    fireEvent.click(screen.getByText('input.contextDetailBack'));

    expect(screen.getByText('input.compact')).toBeInTheDocument();
    expect(screen.getByText('input.refreshAndCompact')).toBeInTheDocument();
    expect(screen.getByText('input.contextDetail')).toBeInTheDocument();
  });

  it('shows an empty state instead of fabricating detail when breakdown is missing', () => {
    useStore.setState({
      compactingSessions: [],
      contextBySession: {
        '/session/a.jsonl': { tokens: 100, window: 200_000, percent: 1 },
      },
    } as never);

    const { container } = render(<ContextRing />);
    fireEvent.click(container.querySelector('button') as HTMLButtonElement);
    fireEvent.click(screen.getByText('input.contextDetail'));

    expect(screen.getByText('input.contextDetailEmpty')).toBeInTheDocument();
    expect(screen.queryByText('input.contextCategory.system')).not.toBeInTheDocument();
  });
});
