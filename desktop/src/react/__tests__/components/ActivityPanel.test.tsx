// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ActivityPanel } from '../../components/ActivityPanel';
import { useStore } from '../../stores';
import { lingxiFetch } from '../../hooks/use-hana-fetch';

let loadPanelData: (() => void) | undefined;
vi.mock('../../hooks/use-panel', () => ({
  usePanel: (_name: string, loadData: () => void) => { loadPanelData = loadData; return { visible: true, close: vi.fn() }; },
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: vi.fn(),
}));

vi.mock('../../hooks/use-config', () => ({
  fetchConfig: vi.fn(() => Promise.resolve({ desk: { heartbeat_master: true } })),
  invalidateConfigCache: vi.fn(),
}));

describe('ActivityPanel', () => {
  beforeEach(() => {
    window.t = ((key: string, vars?: Record<string, string>) => {
      if (key === 'activity.duration') return `耗时 ${vars?.text || ''}`;
      return key;
    }) as typeof window.t;
    vi.mocked(lingxiFetch).mockImplementation((path: string) => {
      if (path === '/api/desk/activities/act_cover/session') {
        return Promise.resolve({
          json: () => Promise.resolve({
            messages: [{ role: 'assistant', content: 'cover 已经生成' }],
          }),
        } as Response);
      }
      return Promise.resolve({
        json: () => Promise.resolve({}),
      } as Response);
    });
    useStore.setState({
      activities: [{
        id: 'act_cover',
        type: 'beautify',
        label: 'Markdown cover',
        status: 'running',
        agentId: 'agent-hana',
        agentName: 'Hanako',
        summary: '正在为 傍晚.md 生成 cover',
        sessionFile: 'act_cover.jsonl',
        startedAt: Date.now(),
      }],
      agents: [{ id: 'agent-hana', name: 'Hanako', yuan: '', isPrimary: true }],
      currentAgentId: 'agent-hana',
      agentName: 'Hanako',
    } as never);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('leaves loading state and renders the background session transcript after expansion', async () => {
    render(<ActivityPanel />);

    await screen.findByText('正在为 傍晚.md 生成 cover');
    fireEvent.click(screen.getByRole('button', { name: 'activity.expand' }));

    expect(screen.getByText('activity.loadingSession')).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText('cover 已经生成')).toBeInTheDocument();
    });
    expect(screen.queryByText('activity.loadingSession')).not.toBeInTheDocument();
  });
  it.each(['unknown', 'cancelled', 'skipped', 'running'])('does not present %s as completed', async (status) => {
    useStore.setState({ activities: [{ id: 'uncertain', type: 'cron', label: '提醒', status, agentId: 'agent-hana', startedAt: Date.now() }] } as never);
    render(<ActivityPanel />);
    expect(screen.queryByText('activity.cronDone')).not.toBeInTheDocument();
    expect(screen.queryByText('activity.patrolDone')).not.toBeInTheDocument();
    const key = status === 'unknown' ? 'activity.unconfirmed' : status === 'running' ? 'common.executing' : `activity.${status}`;
    expect(screen.getAllByText(key).length).toBeGreaterThan(0);
  });

  it('keeps the previous activity list and reports malformed or failed reads', async () => {
    vi.mocked(lingxiFetch).mockResolvedValue(new Response(JSON.stringify({ error: 'unavailable' }), { status: 500 }));
    render(<ActivityPanel />);
    await act(async () => { loadPanelData?.(); });
    expect(await screen.findByRole('alert')).toHaveTextContent('activity.listLoadFailed');
    expect(screen.getByText('正在为 傍晚.md 生成 cover')).toBeInTheDocument();
    expect(useStore.getState().activities).toHaveLength(1);
    expect(screen.queryByText('activity.empty')).not.toBeInTheDocument();
    vi.mocked(lingxiFetch).mockResolvedValue(new Response(JSON.stringify({ error: 'bad shape' }), { status: 200 }));
    await act(async () => { loadPanelData?.(); });
    expect(useStore.getState().activities).toHaveLength(1);
  });

});
