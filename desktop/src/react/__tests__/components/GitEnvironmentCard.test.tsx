// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { GitEnvironmentCard } from '../../components/runtime/GitEnvironmentCard';
import type { GitBranches, GitStatus, GitWorktreeInfo } from '../../utils/git-env-api';

// 卡片行为聚焦：git 数据链路全部 mock 掉
const fetchGitStatusMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitStatus>>();
const fetchGitBranchesMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitBranches>>();
const fetchGitWorktreeInfoMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitWorktreeInfo>>();
const gitCheckoutMock = vi.fn();

vi.mock('../../utils/git-env-api', () => ({
  fetchGitStatus: (dir: string, agentId?: string | null) => fetchGitStatusMock(dir, agentId),
  fetchGitBranches: (dir: string, agentId?: string | null) => fetchGitBranchesMock(dir, agentId),
  fetchGitWorktreeInfo: (dir: string, agentId?: string | null) => fetchGitWorktreeInfoMock(dir, agentId),
  gitCheckout: (...args: unknown[]) => gitCheckoutMock(...args),
  fetchGitFileDiff: vi.fn(),
  gitCommit: vi.fn(),
  gitPush: vi.fn(),
  generateGitCommitMessage: vi.fn(),
}));

const STATUS: GitStatus = {
  isRepo: true,
  currentBranch: 'feat/knowledge-retrieval-research',
  detached: false,
  total: { additions: 73390, deletions: 5000 },
  stagedTotal: { additions: 100, deletions: 0 },
  unstagedTotal: { additions: 4273, deletions: 12 },
  files: [
    { path: 'desktop/src/app.tsx', additions: 4273, deletions: 12, state: 'modified', staged: false },
    { path: 'server/index.ts', additions: 100, deletions: 0, state: 'added', staged: true },
  ],
  hasUpstream: false,
  hasRemote: true,
  ahead: 0,
  behind: 0,
  commitable: true,
  pushable: true,
};

const BRANCHES: GitBranches = {
  isRepo: true,
  detached: false,
  current: 'feat/knowledge-retrieval-research',
  branches: [
    { name: 'feat/knowledge-retrieval-research', current: true, checkedOutElsewhere: false },
    { name: 'main', current: false, checkedOutElsewhere: false },
    { name: 'wt-sep04', current: false, checkedOutElsewhere: true },
  ],
};

const WORKTREE: GitWorktreeInfo = {
  isRepo: true,
  isMain: false,
  name: 'wt-branch',
  branch: 'wt-branch',
  path: '/ws/linked',
  mainPath: '/repo/main',
};

const TABLE: Record<string, string> = {
  'gitEnv.title': '环境信息',
  'gitEnv.changes': '变更',
  'gitEnv.local': '本地',
  'gitEnv.branch': '分支',
  'gitEnv.commitOrPush': '提交或推送',
  'gitEnv.notGitRepo': '非 Git 仓库',
  'gitEnv.loadFailed': '加载失败，点击重试',
  'gitEnv.mainWorktree': '本地主工作树',
  'gitEnv.linkedWorktreeShort': '分支工作树',
  'gitEnv.linkedWorktree': '分支的新工作树：{name}',
  'gitEnv.detachedHead': '分离头指针（{name}）',
  'gitEnv.branchesTitle': '切换分支',
  'gitEnv.checkedOutElsewhere': '该分支已在其他工作树检出',
  'gitEnv.noBranches': '没有本地分支',
  'gitEnv.switchDone': '已切换到 {name}',
  'gitEnv.switchFailed': '切换分支失败',
  'gitEnv.changesTitle': '变更文件',
  'gitEnv.noChanges': '暂无变更',
  'gitEnv.commitTitle': '提交或推送',
  'gitEnv.commitMessagePlaceholder': '提交信息（留空将自动生成）',
  'gitEnv.includeUnstaged': '包含未暂存的更改',
  'gitEnv.btnCommit': '提交',
  'gitEnv.btnCommitPush': '提交并推送',
  'gitEnv.btnPush': '推送',
  'gitEnv.aiGenerating': '正在生成提交信息…',
  'gitEnv.commitDone': '提交完成',
  'gitEnv.pushDone': '推送完成',
  'gitEnv.operationFailed': '操作失败',
};

function makeT() {
  return ((key: string, vars?: Record<string, string | number>) => {
    const template = TABLE[key] ?? key;
    return template.replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
  }) as typeof window.t;
}

describe('GitEnvironmentCard', () => {
  beforeEach(() => {
    window.t = makeT();
    fetchGitStatusMock.mockReset().mockResolvedValue(STATUS);
    fetchGitBranchesMock.mockReset().mockResolvedValue(BRANCHES);
    fetchGitWorktreeInfoMock.mockReset().mockResolvedValue(WORKTREE);
    gitCheckoutMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    useStore.setState({
      deskBasePath: '/ws/linked',
      deskWorkspaceNativeRoot: null,
      deskWorkspaceMountId: null,
      deskWorkspaceLabel: null,
      currentSessionPath: null,
      currentAgentId: null,
      addToast: vi.fn(),
    } as never);
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it('renders nothing when no workspace dir is set', () => {
    useStore.setState({ deskBasePath: null, deskWorkspaceNativeRoot: null } as never);
    const { container } = render(<GitEnvironmentCard />);
    expect(container.querySelector('[data-testid="git-env-card"]')).not.toBeInTheDocument();
  });

  it('shows formatted change totals, worktree kind and current branch on the four rows', async () => {
    render(<GitEnvironmentCard />);

    await waitFor(() => expect(screen.getByTestId('git-env-changes-row')).toHaveTextContent('+73,390'));
    expect(screen.getByTestId('git-env-changes-row')).toHaveTextContent('-5,000');
    expect(screen.getByTestId('git-env-local-row')).toHaveTextContent('分支工作树');
    expect(screen.getByTestId('git-env-branch-row')).toHaveTextContent('feat/knowledge-retrieval-research');
    expect(screen.getByTestId('git-env-commit-row')).toBeInTheDocument();
  });

  it('degrades all rows for a non-git directory', async () => {
    fetchGitStatusMock.mockResolvedValue({ ...STATUS, isRepo: false, files: [], commitable: false, pushable: false });
    fetchGitBranchesMock.mockResolvedValue({ isRepo: false, branches: [], detached: false, current: null });
    fetchGitWorktreeInfoMock.mockResolvedValue({ ...WORKTREE, isRepo: false, isMain: true, name: null });

    render(<GitEnvironmentCard />);

    await waitFor(() => expect(screen.getByTestId('git-env-changes-row')).toHaveTextContent('非 Git 仓库'));
    expect(screen.getByTestId('git-env-changes-row')).toBeDisabled();
    expect(screen.getByTestId('git-env-local-row')).toBeDisabled();
    expect(screen.getByTestId('git-env-branch-row')).toBeDisabled();
    expect(screen.getByTestId('git-env-commit-row')).toBeDisabled();
  });

  it('opens the changes modal from the changes row and lists files with per-file stats', async () => {
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-changes-row')).toHaveTextContent('+73,390'));

    fireEvent.click(screen.getByTestId('git-env-changes-row'));
    expect(await screen.findByText('变更文件')).toBeInTheDocument();
    expect(screen.getByText('desktop/src/app.tsx')).toBeInTheDocument();
    expect(screen.getByText('server/index.ts')).toBeInTheDocument();
  });

  it('expands the local row in place showing the linked worktree and main path', async () => {
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-local-row')).toHaveTextContent('分支工作树'));

    expect(screen.queryByTestId('git-env-local-detail')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('git-env-local-row'));
    expect(screen.getByTestId('git-env-local-detail')).toHaveTextContent('分支的新工作树：wt-branch');
    expect(screen.getByTestId('git-env-local-detail')).toHaveTextContent('/repo/main');
  });

  it('opens the branch popover, marks the current branch and switches on click', async () => {
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-branch-row')).toHaveTextContent('feat/knowledge-retrieval-research'));

    fireEvent.click(screen.getByTestId('git-env-branch-row'));
    expect(await screen.findByTestId('git-branch-main')).toBeInTheDocument();
    // 当前分支与被他树检出的分支都不可点击
    expect(screen.getByTestId('git-branch-feat/knowledge-retrieval-research')).toBeDisabled();
    expect(screen.getByTestId('git-branch-wt-sep04')).toBeDisabled();
    expect(screen.getByTestId('git-branch-main')).toBeEnabled();

    fireEvent.click(screen.getByTestId('git-branch-main'));
    await waitFor(() => expect(gitCheckoutMock).toHaveBeenCalledWith('/ws/linked', 'main', null));
    // 切换成功后整卡刷新
    await waitFor(() => expect(fetchGitStatusMock.mock.calls.length).toBeGreaterThanOrEqual(2));
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已切换到 main', 'success');
  });

  it('opens the commit modal from the commit-or-push row', async () => {
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-changes-row')).toHaveTextContent('+73,390'));

    fireEvent.click(screen.getByTestId('git-env-commit-row'));
    expect(await screen.findByPlaceholderText('提交信息（留空将自动生成）')).toBeInTheDocument();
    expect(screen.getByText('包含未暂存的更改')).toBeInTheDocument();
  });

  it('shows load failure state and retries on click', async () => {
    fetchGitStatusMock.mockRejectedValueOnce(new Error('boom'));
    render(<GitEnvironmentCard />);

    await waitFor(() => expect(screen.getByTestId('git-env-changes-row')).toHaveTextContent('加载失败，点击重试'));
    fireEvent.click(screen.getByTestId('git-env-changes-row'));
    await waitFor(() => expect(fetchGitStatusMock.mock.calls.length).toBeGreaterThanOrEqual(2));
    await waitFor(() => expect(screen.getByTestId('git-env-changes-row')).toHaveTextContent('+73,390'));
  });
});
