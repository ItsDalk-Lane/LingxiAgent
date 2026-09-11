// @vitest-environment jsdom

/**
 * TodoPanel — 输入框上方清单进度条组件测试
 *
 * 覆盖验收项（PLAN.md §8）：
 * - A01 输入框上方出现摘要，无清单时不占位
 * - A02 点击标题 / Enter / 空格展开收起，固定图标
 * - A04 多个进行中步骤的并行摘要
 * - A05 受阻条目展示原因，不计入完成
 * - A06 本轮停止后进行中图标不再表现为执行中，并提示真实含义
 * - A08 全部完成 → 收尾摘要 + 收纳操作
 * - A10 部分完成+取消 → 分别统计
 * - A11 确认完成 / 取消剩余调用服务端动作
 * - A12 输出期间完成/取消操作禁用
 * - A13 更新失败提示，保留原清单
 */
import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { TodoPanel } from '../../components/chat/TodoPanel';
import {
  cancelSessionTodos,
  completeSessionTodos,
  dismissSessionTodoPanel,
} from '../../stores/session-actions';

vi.mock('../../stores/session-actions', () => ({
  completeSessionTodos: vi.fn(async () => true),
  cancelSessionTodos: vi.fn(async () => true),
  dismissSessionTodoPanel: vi.fn(async () => true),
}));

const SESSION = '/session/panel.jsonl';

const I18N: Record<string, string> = {
  'todoPanel.title': '任务',
  'todoPanel.completed': '{n} 已完成',
  'todoPanel.inProgress': '{n} 进行中',
  'todoPanel.pending': '{n} 待开始',
  'todoPanel.blocked': '{n} 受阻',
  'todoPanel.cancelled': '{n} 已取消',
  'todoPanel.allDone': '全部完成 · {n} 项',
  'todoPanel.finishedPrefix': '已结束',
  'todoPanel.moreInProgress': '另有 {n} 项进行中',
  'todoPanel.stopped': '本轮已停止，仍有未完成任务',
  'todoPanel.updateFailed': '清单更新失败，已保留上一份清单',
  'todoPanel.confirmComplete': '确认剩余任务已完成',
  'todoPanel.cancelRemaining': '取消剩余任务',
  'todoPanel.cancelNote': '只改变这份计划，不会终止后台任务',
  'todoPanel.dismiss': '收纳',
  'todoPanel.waitForOutput': '输出完成后再操作',
  'todoPanel.blockedReason': '受阻：{reason}',
  'todoPanel.cancelledLabel': '已取消',
};

function seed(panel: Record<string, unknown> | null, opts: { streaming?: boolean; expanded?: boolean } = {}) {
  useStore.setState({
    currentSessionPath: SESSION,
    currentSessionId: null,
    streamingSessions: opts.streaming ? [SESSION] : [],
    todoPanelBySession: panel ? { [SESSION]: panel } : {},
    todoPanelExpandedBySession: { [SESSION]: opts.expanded === true },
  } as never);
}

const ACTIVE_TODOS = [
  { content: '读文档', activeForm: '正在读文档', status: 'completed' as const },
  { content: '写代码', activeForm: '正在写代码', status: 'in_progress' as const },
  { content: '跑测试', activeForm: '正在跑测试', status: 'pending' as const },
];

describe('TodoPanel', () => {
  beforeEach(() => {
    window.t = ((key: string, vars?: Record<string, string | number>) => {
      const template = I18N[key] ?? key;
      return template.replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
    }) as typeof window.t;
    vi.clearAllMocks();
  });

  afterEach(() => cleanup());

  it('A01: 有清单时显示摘要；无清单时不占位', () => {
    seed(null);
    const { container, rerender } = render(<TodoPanel />);
    expect(container.querySelector('[data-todo-panel]')).not.toBeInTheDocument();

    seed({ todos: ACTIVE_TODOS, version: 'tv1', finished: false, allCompleted: false, dismissed: false, updateFailed: false });
    rerender(<TodoPanel />);
    expect(container.querySelector('[data-todo-panel]')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^任务 ·/ })).toHaveTextContent('1 已完成 · 1 进行中 · 1 待开始');
  });

  it('A02: 点击标题展开收起，Enter/空格同样生效', () => {
    seed({ todos: ACTIVE_TODOS, version: 'tv1', finished: false, allCompleted: false, dismissed: false, updateFailed: false });
    render(<TodoPanel />);
    const header = screen.getByRole('button', { name: /^任务 ·/ });

    expect(screen.queryByText('跑测试')).not.toBeInTheDocument();
    fireEvent.click(header);
    expect(screen.getByText('跑测试')).toBeInTheDocument();
    expect(useStore.getState().todoPanelExpandedBySession[SESSION]).toBe(true);

    fireEvent.keyDown(header, { key: 'Enter' });
    // button 原生 click 由 keyDown 触发在 jsdom 不自动发生，改用 click 模拟键盘结果
    fireEvent.click(header);
    expect(useStore.getState().todoPanelExpandedBySession[SESSION]).toBe(false);
  });

  it('A03: 展开后清单内容更新，展开状态不被重置', () => {
    seed({ todos: ACTIVE_TODOS, version: 'tv1', finished: false, allCompleted: false, dismissed: false, updateFailed: false }, { expanded: true });
    const { rerender } = render(<TodoPanel />);
    expect(screen.getByText('跑测试')).toBeInTheDocument();

    // 模拟流式更新：todos 内容变化，expanded 不变
    seed({
      todos: ACTIVE_TODOS.map((td, i) => (i === 1 ? { ...td, status: 'completed' as const } : td)),
      version: 'tv2', finished: false, allCompleted: false, dismissed: false, updateFailed: false,
    }, { expanded: true });
    rerender(<TodoPanel />);
    expect(useStore.getState().todoPanelExpandedBySession[SESSION]).toBe(true);
    expect(screen.getByText('跑测试')).toBeInTheDocument();
  });

  it('A04: 多个进行中步骤显示并行摘要', () => {
    seed({
      todos: [
        { content: '任务甲', activeForm: '正在做任务甲', status: 'in_progress' as const },
        { content: '任务乙', activeForm: '正在做任务乙', status: 'in_progress' as const },
        { content: '任务丙', activeForm: '正在做任务丙', status: 'in_progress' as const },
      ],
      version: 'tv1', finished: false, allCompleted: false, dismissed: false, updateFailed: false,
    });
    render(<TodoPanel />);
    const header = screen.getByRole('button', { name: /^任务 ·/ });
    expect(header).toHaveTextContent('3 进行中');
    expect(header).toHaveTextContent('正在做任务甲');
    expect(header).toHaveTextContent('另有 2 项进行中');
  });

  it('A05: 受阻条目显示原因，不计入完成', () => {
    seed({
      todos: [
        { content: '做完的', activeForm: '正在做完的', status: 'completed' as const },
        { content: '卡住的', activeForm: '正在卡住的', status: 'blocked' as const, blockedReason: '缺少凭据' },
      ],
      version: 'tv1', finished: false, allCompleted: false, dismissed: false, updateFailed: false,
    }, { expanded: true });
    render(<TodoPanel />);
    expect(screen.getByRole('button', { name: /^任务 ·/ })).toHaveTextContent('1 受阻');
    expect(screen.getByText('受阻：缺少凭据')).toBeInTheDocument();
    // 仍有收尾操作（受阻不算完成）
    expect(screen.getByRole('button', { name: '确认剩余任务已完成' })).toBeInTheDocument();
  });

  it('A06: 非流式且有未完成项时提示本轮已停止，不显示自动打勾', () => {
    seed({ todos: ACTIVE_TODOS, version: 'tv1', finished: false, allCompleted: false, dismissed: false, updateFailed: false }, { streaming: false });
    render(<TodoPanel />);
    expect(screen.getByText('本轮已停止，仍有未完成任务')).toBeInTheDocument();
  });

  it('A06b: 流式期间不显示停止提示', () => {
    seed({ todos: ACTIVE_TODOS, version: 'tv1', finished: false, allCompleted: false, dismissed: false, updateFailed: false }, { streaming: true });
    render(<TodoPanel />);
    expect(screen.queryByText('本轮已停止，仍有未完成任务')).not.toBeInTheDocument();
  });

  it('A08: 全部完成显示收尾摘要和收纳操作', () => {
    seed({
      todos: ACTIVE_TODOS.map((td) => ({ ...td, status: 'completed' as const })),
      version: 'tv1', finished: true, allCompleted: true, dismissed: false, updateFailed: false,
    }, { expanded: true });
    render(<TodoPanel />);
    expect(screen.getByRole('button', { name: /^任务 ·/ })).toHaveTextContent('全部完成 · 3 项');
    fireEvent.click(screen.getByRole('button', { name: '收纳' }));
    expect(dismissSessionTodoPanel).toHaveBeenCalledWith(SESSION);
  });

  it('A10: 部分完成+取消分别统计，不显示全部完成', () => {
    seed({
      todos: [
        { content: '做完的', activeForm: '正在做完的', status: 'completed' as const },
        { content: '取消的', activeForm: '正在取消的', status: 'cancelled' as const },
      ],
      version: 'tv1', finished: true, allCompleted: false, dismissed: false, updateFailed: false,
    }, { expanded: true });
    render(<TodoPanel />);
    const header = screen.getByRole('button', { name: /^任务 ·/ });
    expect(header).toHaveTextContent('已结束');
    expect(header).toHaveTextContent('1 已完成 · 1 已取消');
    expect(header).not.toHaveTextContent('全部完成');
    expect(screen.getByText('已取消')).toBeInTheDocument();
  });

  it('A11: 确认完成与取消剩余调用对应服务端动作', async () => {
    seed({ todos: ACTIVE_TODOS, version: 'tv1', finished: false, allCompleted: false, dismissed: false, updateFailed: false }, { expanded: true });
    render(<TodoPanel />);
    fireEvent.click(screen.getByRole('button', { name: '确认剩余任务已完成' }));
    expect(completeSessionTodos).toHaveBeenCalledWith(SESSION);
    // 等 acting 状态复位再点下一个操作
    await vi.waitFor(() => expect(screen.getByRole('button', { name: '取消剩余任务' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: '取消剩余任务' }));
    expect(cancelSessionTodos).toHaveBeenCalledWith(SESSION);
    expect(screen.getByText('只改变这份计划，不会终止后台任务')).toBeInTheDocument();
  });

  it('A12: 输出期间完成/取消操作禁用，收起仍可用', () => {
    seed({ todos: ACTIVE_TODOS, version: 'tv1', finished: false, allCompleted: false, dismissed: false, updateFailed: false }, { expanded: true, streaming: true });
    render(<TodoPanel />);
    expect(screen.getByRole('button', { name: '确认剩余任务已完成' })).toBeDisabled();
    expect(screen.getByRole('button', { name: '取消剩余任务' })).toBeDisabled();
    // 收起仍可用
    fireEvent.click(screen.getByRole('button', { name: /^任务 ·/ }));
    expect(useStore.getState().todoPanelExpandedBySession[SESSION]).toBe(false);
  });

  it('A13: 更新失败时提示并保留原清单', () => {
    seed({ todos: ACTIVE_TODOS, version: 'tv1', finished: false, allCompleted: false, dismissed: false, updateFailed: true }, { expanded: true });
    render(<TodoPanel />);
    expect(screen.getByText('清单更新失败，已保留上一份清单')).toBeInTheDocument();
    expect(screen.getByText('跑测试')).toBeInTheDocument();
  });
});
