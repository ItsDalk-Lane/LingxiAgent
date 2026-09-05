// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { GitHistoryModal } from '../../components/runtime/GitHistoryModal';
import { useStore } from '../../stores';
import type { GitCommit, GitLogResponse } from '../../utils/git-env-api';

const fetchGitLogMock = vi.fn<(dir: string, agentId?: string | null, limit?: number) => Promise<GitLogResponse>>();

vi.mock('../../utils/git-env-api', () => ({
  fetchGitLog: (dir: string, agentId?: string | null, limit?: number) => fetchGitLogMock(dir, agentId, limit),
}));

const NOW = Date.now();

function makeCommit(overrides: Partial<GitCommit>): GitCommit {
  return {
    hash: 'aaa1110000000000000000000000000000000000',
    shortHash: 'aaa1110',
    subject: 'feat: 示例提交',
    message: 'feat: 示例提交',
    authorName: 'lingxi-dev',
    committedAt: Math.floor(NOW / 1000) - 4 * 3600,
    refs: [],
    parents: [],
    ...overrides,
  };
}

const COMMITS: GitCommit[] = [
  makeCommit({
    hash: 'e35dca2'.padEnd(40, '0'),
    shortHash: 'e35dca2',
    subject: 'feat(desktop): 环境信息卡接入运行信息胶囊',
    message: 'feat(desktop): 环境信息卡接入运行信息胶囊\n\n- 四行卡片与三个弹窗\n- AI 提交信息标题+正文\n- 提交历史泳道图',
    refs: [{ kind: 'head', name: 'feat/pending-sep04' }, { kind: 'remote', name: 'origin/feat/pending-sep04' }],
    parents: ['d6fbd0d3'.padEnd(40, '0')],
  }),
  makeCommit({
    hash: 'd6fbd0d3'.padEnd(40, '0'),
    shortHash: 'd6fbd0d',
    subject: 'feat(desktop): 归档记录按工作台分组并支持整组删除',
    refs: [{ kind: 'tag', name: 'v0.1.33-pre' }],
    parents: ['397bfcd8'.padEnd(40, '0')],
  }),
  makeCommit({
    hash: '397bfcd8'.padEnd(40, '0'),
    shortHash: '397bfcd',
    subject: 'chore(audit): advance VERIFIED_SOURCE_SHA',
    parents: [],
  }),
];

const TABLE: Record<string, string> = {
  'gitEnv.history': '提交记录',
  'gitEnv.noCommits': '暂无提交',
  'gitEnv.loadFailed': '加载失败，点击重试',
  'gitEnv.copyHash': '复制提交 ID',
  'gitEnv.copied': '已复制提交 ID',
  'gitEnv.operationFailed': '操作失败',
  'gitEnv.timeJustNow': '刚刚',
  'gitEnv.timeMinutesAgo': `${'{n}'} 分钟前`,
  'gitEnv.timeHoursAgo': `${'{n}'} 小时前`,
  'gitEnv.timeDaysAgo': `${'{n}'} 天前`,
};

describe('GitHistoryModal', () => {
  beforeEach(() => {
    window.t = (((key: string, vars?: Record<string, string | number>) => {
      const template = TABLE[key] ?? key;
      return template.replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
    }) as typeof window.t);
    fetchGitLogMock.mockReset().mockResolvedValue({ isRepo: true, commits: COMMITS });
    useStore.setState({ addToast: vi.fn() } as never);
  });

  afterEach(() => cleanup());

  it('lists commits one by one with subject, author, relative time and hash chip', async () => {
    render(<GitHistoryModal open onClose={vi.fn()} dir="/ws" />);

    expect(await screen.findByTestId('git-commit-e35dca2')).toBeInTheDocument();
    expect(screen.getByText('feat(desktop): 环境信息卡接入运行信息胶囊')).toBeInTheDocument();
    expect(screen.getByText('feat(desktop): 归档记录按工作台分组并支持整组删除')).toBeInTheDocument();
    expect(screen.getAllByText('lingxi-dev')).toHaveLength(3);
    expect(screen.getAllByText('4 小时前')).toHaveLength(3);
    expect(screen.getByTestId('git-commit-d6fbd0d')).toHaveTextContent('d6fbd0d');
  });

  it('renders ref chips: HEAD·branch, remote and tag', async () => {
    render(<GitHistoryModal open onClose={vi.fn()} dir="/ws" />);

    await screen.findByTestId('git-commit-e35dca2');
    expect(screen.getByText('HEAD · feat/pending-sep04')).toBeInTheDocument();
    expect(screen.getByText('origin/feat/pending-sep04')).toBeInTheDocument();
    expect(screen.getByText('v0.1.33-pre')).toBeInTheDocument();
  });

  it('shows the empty state when the repo has no commits', async () => {
    fetchGitLogMock.mockResolvedValue({ isRepo: true, commits: [] });
    render(<GitHistoryModal open onClose={vi.fn()} dir="/ws" />);
    expect(await screen.findByText('暂无提交')).toBeInTheDocument();
  });

  it('shows an error note when fetching fails', async () => {
    fetchGitLogMock.mockRejectedValue(new Error('boom'));
    render(<GitHistoryModal open onClose={vi.fn()} dir="/ws" />);
    expect(await screen.findByText('加载失败，点击重试')).toBeInTheDocument();
  });

  it('refetches when reopened with a different dir', async () => {
    const { rerender } = render(<GitHistoryModal open onClose={vi.fn()} dir="/ws" />);
    await screen.findByTestId('git-commit-e35dca2');
    rerender(<GitHistoryModal open onClose={vi.fn()} dir="/other" />);
    await waitFor(() => expect(fetchGitLogMock).toHaveBeenLastCalledWith('/other', undefined, 300));
  });

  it('shows the full multi-line commit message via the in-house tooltip on subject hover', async () => {
    render(<GitHistoryModal open onClose={vi.fn()} dir="/ws" />);
    // 先用真实计时器等列表渲染完成，再切假计时器驱动 Tooltip 的 500ms 延迟
    const subject = await screen.findByText('feat(desktop): 环境信息卡接入运行信息胶囊');
    vi.useFakeTimers();
    try {
      expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
      fireEvent.mouseEnter(subject);
      act(() => { vi.advanceTimersByTime(600); });
      const tooltip = screen.getByRole('tooltip');
      expect(tooltip).toHaveTextContent('AI 提交信息标题+正文');
      expect(tooltip).toHaveTextContent('提交历史泳道图');
      fireEvent.mouseLeave(subject);
      expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it('shows the full hash via tooltip on the hash chip', async () => {
    render(<GitHistoryModal open onClose={vi.fn()} dir="/ws" />);
    const chip = await screen.findByTestId('git-hash-e35dca2');
    vi.useFakeTimers();
    try {
      fireEvent.mouseEnter(chip);
      act(() => { vi.advanceTimersByTime(600); });
      expect(screen.getByRole('tooltip')).toHaveTextContent('e35dca2'.padEnd(40, '0'));
      fireEvent.mouseLeave(chip);
      expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it('copies the full commit hash on chip click with feedback', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    render(<GitHistoryModal open onClose={vi.fn()} dir="/ws" />);

    const chip = await screen.findByTestId('git-hash-e35dca2');
    expect(chip).toHaveTextContent('e35dca2');
    fireEvent.click(chip);

    await waitFor(() => expect(writeText).toHaveBeenCalledWith('e35dca2'.padEnd(40, '0')));
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已复制提交 ID', 'success');
    // 复制反馈：徽标短暂显示 ✓
    expect(screen.getByTestId('git-hash-e35dca2')).toHaveTextContent('✓');
  });
});
