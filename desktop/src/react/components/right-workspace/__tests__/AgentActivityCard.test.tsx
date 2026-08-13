/**
 * @vitest-environment jsdom
 */
import React from 'react';
import { afterEach, describe, it, expect, vi } from 'vitest';
import { cleanup, render, fireEvent, act } from '@testing-library/react';
import { AgentActivityCard } from '../AgentActivityCard';

const controlMocks = vi.hoisted(() => ({
  stopSubagentProcess: vi.fn(async () => ({ ok: true })),
}));

const navigationMocks = vi.hoisted(() => ({
  navigateToChatCard: vi.fn(),
}));

vi.mock('../../../services/background-process-control', () => controlMocks);
vi.mock('../../../services/chat-card-navigation', () => navigationMocks);

// mock store：组件用 useStore（currentSessionPath + selectAgentActivities + agents）
// 子组件展开时走 useStore.getState().setSubagentPreviewSessionPath
const mockState: any = {
  currentSessionPath: '/s/a.jsonl',
  agentActivitiesBySession: {},
  agents: [],
  setSubagentPreviewSessionPath: vi.fn(),
};
vi.mock('../../../stores', () => {
  const useStore: any = (selector: (s: any) => any) => selector(mockState);
  useStore.getState = () => mockState;
  return { useStore };
});

// 右侧栏不再挂载详情；这个 mock 用来钉住它不会被错误重新引入。
vi.mock('../../chat/SubagentSessionPreview', () => ({
  SubagentSessionPreview: (props: any) => (
    <div
      data-testid="preview"
      data-session={props.sessionPath ?? ''}
      data-task={props.taskId}
      data-stream={props.streamStatus}
    />
  ),
}));

const mk = (over: any) => ({
  id: 'x', kind: 'subagent', status: 'running', sessionPath: '/s/a.jsonl',
  agentId: null, agentName: null, summary: 's', childSessionPath: null, startedAt: 1, finishedAt: null, ...over,
});

describe('AgentActivityCard', () => {
  afterEach(() => cleanup());
  it('无活动时返回 null（desk 撑满）', () => {
    mockState.currentSessionPath = '/s/a.jsonl';
    mockState.agentActivitiesBySession = {};
    const { container } = render(<AgentActivityCard />);
    expect(container.querySelector('.universal-card')).toBeNull();
  });

  it('只渲染当前 session 中仍在运行的 subagent', () => {
    mockState.agentActivitiesBySession = {
      '/s/a.jsonl': [
        mk({ id: 'd2', status: 'done', agentName: '毛毛', summary: '调研完成', startedAt: 1000, finishedAt: 2000 }),
        mk({ id: 'd1', status: 'running', agentName: '小黎', summary: '点评咖啡', startedAt: 3000 }),
        mk({ id: 'wf', kind: 'workflow', status: 'running', summary: 'workflow-only', startedAt: 4000 }),
      ],
      '/s/b.jsonl': [mk({ id: 'other', agentName: '别的', summary: '别的对话', sessionPath: '/s/b.jsonl', startedAt: 9000 })],
    };
    const { container } = render(<AgentActivityCard />);
    const rows = container.querySelectorAll('[data-status]');
    expect(rows).toHaveLength(1);
    expect(rows[0].getAttribute('data-status')).toBe('running');
    expect(container.textContent).toContain('小黎');
    expect(container.textContent).toContain('点评咖啡');
    expect(container.textContent).not.toContain('毛毛');
    expect(container.textContent).not.toContain('别的对话');
    expect(container.textContent).not.toContain('workflow-only');
  });

  it('reads current session activity from the session id bucket', () => {
    mockState.currentSessionPath = '/s/a.jsonl';
    mockState.currentSessionId = 'sess_a';
    mockState.sessionLocatorsById = { sess_a: { path: '/s/a.jsonl' } };
    mockState.agentActivitiesBySession = {
      sess_a: [
        mk({ id: 'd1', status: 'running', agentName: '小黎', summary: '点评咖啡', startedAt: 3000 }),
      ],
      '/s/a.jsonl': [
        mk({ id: 'legacy', status: 'running', agentName: '旧桶', summary: '旧 path bucket', startedAt: 1000 }),
      ],
    };

    const { container } = render(<AgentActivityCard />);

    expect(container.textContent).toContain('点评咖啡');
    expect(container.textContent).not.toContain('旧 path bucket');
    mockState.currentSessionId = null;
    mockState.sessionLocatorsById = {};
  });

  it('标题跳转到对话卡，右侧不展开详情，停止按钮终止同会话任务', () => {
    controlMocks.stopSubagentProcess.mockClear();
    navigationMocks.navigateToChatCard.mockClear();
    mockState.currentSessionId = 'sess_a';
    mockState.agentActivitiesBySession = {
      '/s/a.jsonl': [mk({ id: 't1', status: 'running', agentId: 'ag1', agentName: '小黎', summary: '点评咖啡', childSessionPath: '/s/child.jsonl' })],
    };
    const { container, queryByTestId, getByRole } = render(<AgentActivityCard />);
    expect(queryByTestId('preview')).toBeNull();

    fireEvent.click(container.querySelector('[data-subagent-title="t1"]') as HTMLElement);
    expect(navigationMocks.navigateToChatCard).toHaveBeenCalledWith({ kind: 'subagent', ids: ['t1'], sessionPath: '/s/a.jsonl' });
    expect(queryByTestId('preview')).toBeNull();

    fireEvent.click(getByRole('button', { name: 'rightWorkspace.subagent.stop' }));
    expect(controlMocks.stopSubagentProcess).toHaveBeenCalledWith({
      sessionId: 'sess_a',
      sessionPath: '/s/a.jsonl',
      taskId: 't1',
    });
    expect(container.querySelector('[aria-expanded]')).toBeNull();
  });

  it('resets the stopping state on a fallback timer when the authoritative event never arrives', async () => {
    controlMocks.stopSubagentProcess.mockClear();
    mockState.currentSessionId = 'sess_a';
    mockState.agentActivitiesBySession = {
      '/s/a.jsonl': [mk({ id: 't1', status: 'running', agentName: '小黎', summary: '点评咖啡' })],
    };
    const { getByRole } = render(<AgentActivityCard />);

    vi.useFakeTimers();
    try {
      fireEvent.click(getByRole('button', { name: 'rightWorkspace.subagent.stop' }));
      await act(async () => {});
      expect(controlMocks.stopSubagentProcess).toHaveBeenCalled();
      expect((getByRole('button', { name: 'rightWorkspace.process.stopping' }) as HTMLButtonElement).disabled).toBe(true);

      await act(async () => {
        vi.advanceTimersByTime(30_000);
      });
      expect((getByRole('button', { name: 'rightWorkspace.subagent.stop' }) as HTMLButtonElement).disabled).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it('无当前 session 时返回 null', () => {
    mockState.currentSessionPath = null;
    mockState.agentActivitiesBySession = { '/s/a.jsonl': [mk({ id: 'x' })] };
    const { container } = render(<AgentActivityCard />);
    expect(container.querySelector('.universal-card')).toBeNull();
    mockState.currentSessionPath = '/s/a.jsonl'; // 复位
  });
});
