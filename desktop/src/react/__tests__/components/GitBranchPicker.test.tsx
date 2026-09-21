// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { GitBranchPicker } from '../../components/runtime/GitBranchPicker';
import { resetGitEnvCache } from '../../hooks/use-git-env';
import type { GitBranches, GitStatus, GitWorktrees } from '../../utils/git-env-api';

// 入口行为聚焦：git 数据链路与工作台注册/切换全部 mock 掉
const fetchGitStatusMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitStatus>>();
const fetchGitBranchesMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitBranches>>();
const fetchGitWorktreesMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitWorktrees>>();
const fetchGitStashesMock = vi.fn();
const gitCheckoutMock = vi.fn();
const gitCreateBranchMock = vi.fn();
const gitCreateWorktreeMock = vi.fn();
const fetchGitLogMock = vi.fn();
const createLocalStudioWorkspaceFromFolderMock = vi.fn();
const applyStudioWorkspaceMock = vi.fn();

vi.mock('../../stores/desk-actions', () => ({
  createLocalStudioWorkspaceFromFolder: (...args: unknown[]) => createLocalStudioWorkspaceFromFolderMock(...args),
  applyStudioWorkspace: (...args: unknown[]) => applyStudioWorkspaceMock(...args),
}));

vi.mock('../../utils/git-env-api', () => ({
  fetchGitStatus: (dir: string, agentId?: string | null) => fetchGitStatusMock(dir, agentId),
  fetchGitBranches: (dir: string, agentId?: string | null) => fetchGitBranchesMock(dir, agentId),
  fetchGitWorktrees: (dir: string, agentId?: string | null) => fetchGitWorktreesMock(dir, agentId),
  fetchGitStashes: (...args: unknown[]) => fetchGitStashesMock(...args),
  gitCheckout: (...args: unknown[]) => gitCheckoutMock(...args),
  gitCreateBranch: (...args: unknown[]) => gitCreateBranchMock(...args),
  gitCreateWorktree: (...args: unknown[]) => gitCreateWorktreeMock(...args),
  fetchGitFileDiff: vi.fn(),
  fetchGitLog: (...args: unknown[]) => fetchGitLogMock(...args),
  fetchGitLogStats: vi.fn().mockResolvedValue({ isRepo: true, stats: {} }),
  gitCommit: vi.fn(),
  gitAmend: vi.fn(),
  gitStage: vi.fn(),
  gitUnstage: vi.fn(),
  gitPush: vi.fn(),
  gitPull: vi.fn(),
  gitDiscard: vi.fn(),
  gitStash: vi.fn(),
  generateGitCommitMessage: vi.fn(),
}));

const STATUS: GitStatus = {
  isRepo: true,
  currentBranch: 'feat/knowledge-retrieval-research',
  detached: false,
  total: { additions: 73390, deletions: 5000 },
  stagedTotal: { additions: 100, deletions: 0 },
  unstagedTotal: { additions: 4273, deletions: 12 },
  files: [],
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

const WORKTREES: GitWorktrees = {
  isRepo: true,
  root: '/repo/worktrees',
  mainPath: '/repo/main',
  worktrees: [
    {
      path: '/repo/main', head: 'b'.repeat(40), branch: 'main',
      detached: false, bare: false, isMain: true, current: false,
    },
    {
      path: '/ws/linked', head: 'a'.repeat(40), branch: 'feat/knowledge-retrieval-research',
      detached: false, bare: false, isMain: false, current: true,
    },
    {
      path: '/repo/worktrees/wt-sep04', head: 'c'.repeat(40), branch: 'wt-sep04',
      detached: false, bare: false, isMain: false, current: false,
    },
    {
      path: '/repo/worktrees/orphan', head: 'd'.repeat(40), branch: null,
      detached: true, bare: false, isMain: false, current: false,
    },
  ],
};

const TABLE: Record<string, string> = {
  'common.cancel': '取消',
  'common.close': '关闭',
  'gitEnv.changes': '变更',
  'gitEnv.branch': '分支',
  'gitEnv.history': '提交记录',
  'gitEnv.notGitRepo': '非 Git 仓库',
  'gitEnv.detachedHead': '分离头指针（{name}）',
  'gitEnv.checkedOutElsewhere': '该分支已在其他工作树检出',
  'gitEnv.noBranches': '没有本地分支',
  'gitEnv.switchDone': '已切换到 {name}',
  'gitEnv.switchFailed': '切换分支失败',
  'gitEnv.newBranch': '新建分支…',
  'gitEnv.newBranchPlaceholder': '新分支名称',
  'gitEnv.createBranch': '创建并切换',
  'gitEnv.createBranchDone': '已创建并切换到 {name}',
  'gitEnv.branchExists': '分支已存在',
  'gitEnv.worktreeMainTag': '主',
  'gitEnv.worktreeLinkedTag': '独',
  'gitEnv.worktreeNoBranch': '未连接分支',
  'gitEnv.worktreeRow': '新建工作树',
  'gitEnv.worktreeDesc': '在当前项目所处的同一父级目录（{root}）创建隔离 worktree。',
  'gitEnv.worktreeDone': '已在 {name} 中打开新会话',
  'gitEnv.worktreeOpenFailed': '工作树已建好，但注册成工作台失败',
  'gitEnv.invalidWorktreeName': '名称不合法',
};

function makeT() {
  return ((key: string, vars?: Record<string, string | number>) => {
    const template = TABLE[key] ?? key;
    return template.replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
  }) as typeof window.t;
}

describe('GitBranchPicker', () => {
  beforeEach(() => {
    resetGitEnvCache();
    window.t = makeT();
    fetchGitStatusMock.mockReset().mockResolvedValue(STATUS);
    fetchGitBranchesMock.mockReset().mockResolvedValue(BRANCHES);
    fetchGitWorktreesMock.mockReset().mockResolvedValue(WORKTREES);
    fetchGitStashesMock.mockReset().mockResolvedValue({ isRepo: true, stashes: [] });
    gitCheckoutMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    gitCreateBranchMock.mockReset().mockResolvedValue({ httpOk: true, ok: true, branch: 'feat/brand-new' });
    gitCreateWorktreeMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    fetchGitLogMock.mockReset().mockResolvedValue({
      isRepo: true,
      commits: [
        {
          hash: 'aaa111'.padEnd(40, '0'), shortHash: 'aaa111', subject: 'feat: 头号提交',
          authorName: 'lingxi-dev', committedAt: Math.floor(Date.now() / 1000),
          refs: [{ kind: 'head', name: 'feat/demo' }], parents: [],
        },
      ],
    });
    createLocalStudioWorkspaceFromFolderMock.mockReset().mockResolvedValue(null);
    applyStudioWorkspaceMock.mockReset().mockResolvedValue(undefined);
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
  });

  it('renders nothing without a dir or when it is not a git repo', async () => {
    useStore.setState({ deskBasePath: null, deskWorkspaceNativeRoot: null } as never);
    const { container } = render(<GitBranchPicker />);
    expect(container.querySelector('[data-testid="branch-picker-pill"]')).not.toBeInTheDocument();

    useStore.setState({ deskBasePath: '/plain-folder' } as never);
    fetchGitStatusMock.mockResolvedValue({ ...STATUS, isRepo: false, commitable: false, pushable: false });
    const second = render(<GitBranchPicker />);
    await waitFor(() => expect(fetchGitStatusMock).toHaveBeenCalled());
    expect(second.container.querySelector('[data-testid="branch-picker-pill"]')).not.toBeInTheDocument();
  });

  it('shows the current branch on the picker and merges worktree badges in the menu', async () => {
    render(<GitBranchPicker />);
    const pill = await screen.findByTestId('branch-picker-pill');
    // IDE 风控件：图标 + 分支名 + ▾，不再有「分支」文字标签
    expect(pill).not.toHaveTextContent('分支');
    expect(pill).toHaveTextContent('feat/knowledge-retrieval-research');

    fireEvent.click(pill);

    // 当前分支检出在独立工作树 → 「独」标志；main 在主工作树 → 「主」标志
    expect(screen.getByTestId('git-branch-feat/knowledge-retrieval-research')).toHaveTextContent('独');
    expect(screen.getByTestId('git-branch-main')).toHaveTextContent('主');
    // 没有工作树的分支（若数据如此）不带标志；未连接分支的工作树单列末尾
    expect(screen.getByTestId('git-branch-worktree-no-branch')).toHaveTextContent('未连接分支');
    // 路径不直接展示
    expect(screen.queryByText('/repo/main')).not.toBeInTheDocument();
  });

  it('lists the three bottom actions in order: worktree, branch, history', async () => {
    render(<GitBranchPicker />);
    fireEvent.click(await screen.findByTestId('branch-picker-pill'));

    const createRow = screen.getByTestId('git-branch-create-row');
    const worktreeBtn = within(createRow).getByTestId('git-env-worktree-create');
    const historyBtn = within(createRow).getByTestId('git-branch-history');
    const branchToggle = within(createRow).getByTestId('git-branch-create-toggle');
    // 顺序：新建工作树 → 新建分支 → 提交记录（提交记录在最底部）
    expect(worktreeBtn.compareDocumentPosition(branchToggle) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(branchToggle.compareDocumentPosition(historyBtn) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(worktreeBtn).toHaveTextContent('新建工作树');
    expect(historyBtn).toHaveTextContent('提交记录');
  });

  it('switches a branch from the menu and refreshes shared data', async () => {
    render(<GitBranchPicker />);
    fireEvent.click(await screen.findByTestId('branch-picker-pill'));

    expect(screen.getByTestId('git-branch-feat/knowledge-retrieval-research')).toBeDisabled();
    expect(screen.getByTestId('git-branch-wt-sep04')).toBeDisabled();
    fireEvent.click(screen.getByTestId('git-branch-main'));

    await waitFor(() => expect(gitCheckoutMock).toHaveBeenCalledWith('/ws/linked', 'main', null));
    await waitFor(() => expect(fetchGitStatusMock.mock.calls.length).toBeGreaterThanOrEqual(2));
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已切换到 main', 'success');
  });

  it('creates a branch from the menu', async () => {
    render(<GitBranchPicker />);
    fireEvent.click(await screen.findByTestId('branch-picker-pill'));

    fireEvent.click(screen.getByTestId('git-branch-create-toggle'));
    fireEvent.change(screen.getByTestId('git-branch-create-input'), { target: { value: 'feat/brand-new' } });
    fireEvent.click(screen.getByTestId('git-branch-create-submit'));

    await waitFor(() => expect(gitCreateBranchMock).toHaveBeenCalledWith('/ws/linked', 'feat/brand-new', undefined, null));
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已创建并切换到 feat/brand-new', 'success');
  });

  it('creates a worktree from the menu bottom and opens a session in it', async () => {
    gitCreateWorktreeMock.mockResolvedValue({
      httpOk: true, ok: true,
      path: '/repo/worktrees/fix-login-race', branch: 'wt/fix-login-race',
    });
    createLocalStudioWorkspaceFromFolderMock.mockResolvedValue({
      mountId: 'ws-fix-login-race', label: 'fix-login-race', nativeRootPath: '/repo/worktrees/fix-login-race',
    });

    render(<GitBranchPicker />);
    fireEvent.click(await screen.findByTestId('branch-picker-pill'));
    fireEvent.click(screen.getByTestId('git-env-worktree-create'));

    const nameInput = await screen.findByTestId('git-worktree-name');
    expect(screen.getByTestId('git-worktree-desc')).toHaveTextContent('/repo/worktrees');
    expect((screen.getByTestId('git-worktree-base') as HTMLSelectElement).value).toBe('feat/knowledge-retrieval-research');
    expect(screen.getByTestId('git-worktree-create')).toBeDisabled();

    fireEvent.change(nameInput, { target: { value: 'fix-login-race' } });
    fireEvent.click(screen.getByTestId('git-worktree-create'));

    await waitFor(() => expect(gitCreateWorktreeMock).toHaveBeenCalledWith('/ws/linked', {
      name: 'fix-login-race', base: 'feat/knowledge-retrieval-research', agentId: null,
    }));
    await waitFor(() => expect(createLocalStudioWorkspaceFromFolderMock).toHaveBeenCalledWith('/repo/worktrees/fix-login-race'));
    await waitFor(() => expect(applyStudioWorkspaceMock).toHaveBeenCalled());
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已在 wt/fix-login-race 中打开新会话', 'success');
    expect(gitCheckoutMock).not.toHaveBeenCalled();
  });

  it('opens the commit history modal from the menu bottom', async () => {
    render(<GitBranchPicker />);
    fireEvent.click(await screen.findByTestId('branch-picker-pill'));
    fireEvent.click(screen.getByTestId('git-branch-history'));

    expect(await screen.findByText('feat: 头号提交')).toBeInTheDocument();
    expect(fetchGitLogMock).toHaveBeenCalledWith('/ws/linked', null, 300);
  });

  it('re-renders instantly from the shared snapshot cache', async () => {
    // 第一次挂载：取数写缓存
    const first = render(<GitBranchPicker />);
    await screen.findByTestId('branch-picker-pill');
    await waitFor(() => expect(fetchGitStatusMock.mock.calls.length).toBeGreaterThanOrEqual(1));
    first.unmount();

    // 第二次挂载：status 挂起不返回 → 胶囊仍须来自缓存秒出（stale-while-revalidate）
    let resolveSecond!: (v: GitStatus) => void;
    fetchGitStatusMock.mockReset().mockImplementation(() => new Promise<GitStatus>(resolve => { resolveSecond = resolve; }));
    render(<GitBranchPicker />);
    const pill = await screen.findByTestId('branch-picker-pill');
    expect(pill).toHaveTextContent('feat/knowledge-retrieval-research');

    // 后台刷新返回新分支 → 胶囊随之更新
    resolveSecond({ ...STATUS, currentBranch: 'feat/second-mount' });
    await screen.findByText('feat/second-mount');
  });
});
