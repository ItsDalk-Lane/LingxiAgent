// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { RuntimeInfoCapsule } from '../../components/runtime/RuntimeInfoCapsule';

// 各运行卡有独立测试；这里用无文案 marker 聚焦胶囊的收起/展开与装载行为
vi.mock('../../components/right-workspace/SessionTodoCard', () => ({
  SessionTodoCard: () => <section data-testid="capsule-todo" />,
}));
vi.mock('../../components/right-workspace/TerminalCard', () => ({
  TerminalCard: () => <section data-testid="capsule-terminal" />,
}));
vi.mock('../../components/right-workspace/WorkflowCard', () => ({
  WorkflowCard: () => <section data-testid="capsule-workflow" />,
}));
vi.mock('../../components/right-workspace/AgentActivityCard', () => ({
  AgentActivityCard: () => <section data-testid="capsule-agent" />,
}));
vi.mock('../../components/right-workspace/SessionStatusCard', () => ({
  SessionStatusCard: () => <section data-testid="capsule-status" />,
}));
vi.mock('../../components/desk/DeskEditor', () => ({
  JianEditor: () => <div data-testid="capsule-jian" data-desk-editor="" />,
}));

describe('RuntimeInfoCapsule', () => {
  beforeEach(() => {
    window.t = ((key: string, vars?: Record<string, string | number>) => {
      const table: Record<string, string> = {
        'runtimeCapsule.title': '运行信息',
        'runtimeCapsule.expand': '展开运行信息',
        'runtimeCapsule.collapse': '收起运行信息',
        'runtimeCapsule.running': `${vars?.n ?? 0} 项进行中`,
        'desk.jianLabel': '笺',
      };
      return table[key] || key;
    }) as typeof window.t;
    useStore.setState({
      currentSessionPath: '/sessions/main.jsonl',
      currentSessionId: 'sess-main',
      terminalsBySession: {},
      agentActivitiesBySession: {},
    } as never);
  });

  afterEach(() => {
    cleanup();
  });

  it('renders collapsed as a pill outside the document flow of the transcript', () => {
    const { container } = render(<RuntimeInfoCapsule />);

    const root = container.querySelector('[data-runtime-capsule]');
    expect(root).toHaveAttribute('data-expanded', 'false');
    expect(screen.getByRole('button', { name: '展开运行信息' })).toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('expands into one unified container holding jian, todo, terminal, workflow, agent and status', () => {
    const { container } = render(<RuntimeInfoCapsule />);

    fireEvent.click(screen.getByRole('button', { name: '展开运行信息' }));

    // 单一容器：胶囊自身长成统一容器（dialog 在同一 root 内，无独立弹出面板）
    const root = container.querySelector('[data-runtime-capsule]');
    expect(root).toHaveAttribute('data-expanded', 'true');
    const panel = screen.getByRole('dialog', { name: '运行信息' });
    expect(root).toContainElement(panel);

    expect(screen.getByTestId('capsule-jian')).toBeInTheDocument();
    expect(screen.getByTestId('capsule-todo')).toBeInTheDocument();
    expect(screen.getByTestId('capsule-terminal')).toBeInTheDocument();
    expect(screen.getByTestId('capsule-workflow')).toBeInTheDocument();
    expect(screen.getByTestId('capsule-agent')).toBeInTheDocument();
    expect(screen.getByTestId('capsule-status')).toBeInTheDocument();
  });

  it('collapses again on pill click and on outside mousedown', () => {
    render(<RuntimeInfoCapsule />);

    fireEvent.click(screen.getByRole('button', { name: '展开运行信息' }));
    fireEvent.click(screen.getByRole('button', { name: '收起运行信息' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '展开运行信息' }));
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('summarizes running terminals, workflows and subagents of the current session on the pill', () => {
    useStore.setState({
      terminalsBySession: {
        '/sessions/main.jsonl': [
          { terminalId: 't1', status: 'running' },
          { terminalId: 't2', status: 'done' },
        ],
      },
      agentActivitiesBySession: {
        '/sessions/main.jsonl': [
          { id: 'w1', kind: 'workflow', status: 'running' },
          { id: 'a1', kind: 'subagent', status: 'running' },
          { id: 'a2', kind: 'subagent', status: 'done' },
        ],
      },
    } as never);

    render(<RuntimeInfoCapsule />);

    expect(screen.getByRole('button', { name: '展开运行信息' })).toHaveTextContent('3 项进行中');
  });
});
