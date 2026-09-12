// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { GitEnvironmentCard } from '../../components/runtime/GitEnvironmentCard';
import type { GitBranches, GitStatus, GitWorktreeInfo, GitWorktrees } from '../../utils/git-env-api';

// 卡片行为聚焦：git 数据链路全部 mock 掉
const fetchGitStatusMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitStatus>>();
const fetchGitBranchesMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitBranches>>();
const fetchGitWorktreeInfoMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitWorktreeInfo>>();
const fetchGitWorktreesMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitWorktrees>>();
const fetchGitStashesMock = vi.fn();
const gitCheckoutMock = vi.fn();
const gitCreateBranchMock = vi.fn();
const gitCreateWorktreeMock = vi.fn();

const fetchGitLogMock = vi.fn();

// 工作台注册/切换（worktree 建成后开新会话）不在本卡行为范围内，直接 mock 掉
const createLocalStudioWorkspaceFromFolderMock = vi.fn();
const applyStudioWorkspaceMock = vi.fn();

vi.mock('../../stores/desk-actions', () => ({
  createLocalStudioWorkspaceFromFolder: (...args: unknown[]) => createLocalStudioWorkspaceFromFolderMock(...args),
  applyStudioWorkspace: (...args: unknown[]) => applyStudioWorkspaceMock(...args),
}));

vi.mock('../../utils/git-env-api', () => ({
  fetchGitStatus: (dir: string, agentId?: string | null) => fetchGitStatusMock(dir, agentId),
  fetchGitBranches: (dir: string, agentId?: string | null) => fetchGitBranchesMock(dir, agentId),
  fetchGitWorktreeInfo: (dir: string, agentId?: string | null) => fetchGitWorktreeInfoMock(dir, agentId),
  fetchGitWorktrees: (dir: string, agentId?: string | null) => fetchGitWorktreesMock(dir, agentId),
  fetchGitStashes: (...args: unknown[]) => fetchGitStashesMock(...args),
  gitCheckout: (...args: unknown[]) => gitCheckoutMock(...args),
  gitCreateBranch: (...args: unknown[]) => gitCreateBranchMock(...args),
  gitCreateWorktree: (...args: unknown[]) => gitCreateWorktreeMock(...args),
  fetchGitFileDiff: vi.fn(),
  fetchGitLog: (...args: unknown[]) => fetchGitLogMock(...args),
  gitCommit: vi.fn(),
  gitPush: vi.fn(),
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
      path: '/ws/linked', head: 'a'.repeat(40), branch: 'wt-branch',
      detached: false, bare: false, isMain: false, current: true,
    },
  ],
};

const TABLE: Record<string, string> = {
  'common.cancel': '取消',
  'common.close': '关闭',
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
  'gitEnv.newBranch': '新建分支…',
  'gitEnv.newBranchPlaceholder': '新分支名称',
  'gitEnv.createBranch': '创建并切换',
  'gitEnv.createBranchDone': '已创建并切换到 {name}',
  'gitEnv.createBranchFailed': '创建分支失败',
  'gitEnv.branchExists': '分支已存在',
  'gitEnv.worktreesTitle': '其他工作树',
  'gitEnv.worktreeMainTag': '主',
  'gitEnv.worktreeCurrentTag': '当前',
  'gitEnv.worktreeRow': '新建工作树',
  'gitEnv.worktreeTitle': '在 worktree 中开始新会话',
  'gitEnv.worktreeDesc': '在当前项目所处的同一父级目录（{root}）创建隔离 worktree。',
  'gitEnv.worktreeNameLabel': 'worktree 名称',
  'gitEnv.worktreeNamePlaceholder': '例如 fix-login-race',
  'gitEnv.invalidWorktreeName': '名称不合法',
  'gitEnv.worktreeBaseLabel': '基线分支',
  'gitEnv.worktreeBaseHead': '当前 HEAD',
  'gitEnv.worktreeCreate': '创建并开始会话',
  'gitEnv.worktreeCreating': '正在创建…',
  'gitEnv.worktreeDone': '已在 {name} 中打开新会话',
  'gitEnv.worktreeFailed': '创建工作树失败',
  'gitEnv.worktreeOpenFailed': '工作树已建好，但注册成工作台失败',
  'gitEnv.worktreeDirExists': '同名目录已存在',
  'gitEnv.worktreeBranchExists': '分支已存在',
  'gitEnv.branchMissing': '基线分支不存在',
  'gitEnv.changesTitle': '变更文件',
  'gitEnv.noChanges': '暂无变更',
  'gitEnv.commitTitle': '提交或推送',
  'gitEnv.commitMessagePlaceholder': '提交信息（留空将自动生成）',
  'gitEnv.includeUnstaged': '包含未暂存的更改',
  'gitEnv.btnCommit': '提交',
  'gitEnv.btnCommitPush': '提交并推送',
  'gitEnv.btnPush': '推送',
  'gitEnv.genMessage': 'AI 生成',
  'gitEnv.aiGenerating': '正在生成提交信息…',
  'gitEnv.aiGeneratingShort': '生成中…',
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
    fetchGitWorktreesMock.mockReset().mockResolvedValue(WORKTREES);
    fetchGitStashesMock.mockReset().mockResolvedValue({ isRepo: true, stashes: [] });
    gitCheckoutMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    gitCreateBranchMock.mockReset().mockResolvedValue({ httpOk: true, ok: true, branch: 'feat/brand-new' });
    gitCreateWorktreeMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    createLocalStudioWorkspaceFromFolderMock.mockReset().mockResolvedValue(null);
    applyStudioWorkspaceMock.mockReset().mockResolvedValue(undefined);
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

  it('expands the local row in place without any section titles', async () => {
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-local-row')).toHaveTextContent('分支工作树'));

    expect(screen.queryByTestId('git-env-local-detail')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('git-env-local-row'));

    const detail = screen.getByTestId('git-env-local-detail');
    // 标题行已移除：既不再有「本地主工作树」抬头，也不再有「全部工作树」分组标题
    expect(detail).not.toHaveTextContent('分支的新工作树');
    expect(detail).not.toHaveTextContent('本地主工作树');
    expect(detail).not.toHaveTextContent('全部工作树');
    // 列表直接呈现 worktree 本身
    expect(detail).toHaveTextContent('wt-branch');
    expect(detail).toHaveTextContent('/ws/linked');
  });

  it('lists every worktree as name-over-path two-line items', async () => {
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-local-row')).toHaveTextContent('分支工作树'));

    fireEvent.click(screen.getByTestId('git-env-local-row'));
    const list = screen.getByTestId('git-env-worktree-list');
    // 两项：主工作树（main）与当前链接工作树（wt-branch）
    const mainItem = screen.getByTestId('git-worktree-item-main');
    const linkedItem = screen.getByTestId('git-worktree-item-wt-branch');

    expect(mainItem).toHaveTextContent('主');
    expect(linkedItem).toHaveTextContent('当前');
    expect(list).toHaveTextContent('/repo/main');
    expect(list).toHaveTextContent('/ws/linked');

    // 名称与路径是各自独立的节点（上下两行），且用的是不同的样式类
    for (const item of [mainItem, linkedItem]) {
      const name = within(item).getByTestId('git-worktree-item-name');
      const path = within(item).getByTestId('git-worktree-item-path');
      expect(name).not.toBe(path);
      expect(name.className).not.toBe(path.className);
      // 结构上确为上下两行：名称行（含标记）在前，路径行紧随其后
      expect(item.firstElementChild).toBe(name.parentElement);
      expect(name.parentElement?.nextElementSibling).toBe(path);
    }

    expect(within(mainItem).getByTestId('git-worktree-item-name')).toHaveTextContent('main');
    expect(within(linkedItem).getByTestId('git-worktree-item-path')).toHaveTextContent('/ws/linked');
    expect(fetchGitWorktreesMock).toHaveBeenCalledWith('/ws/linked', null);
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

  it('creates a branch straight from the branch popover and refreshes the card', async () => {
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-branch-row')).toHaveTextContent('feat/knowledge-retrieval-research'));

    fireEvent.click(screen.getByTestId('git-env-branch-row'));
    // 浮层无标题行：第一项就是分支本身
    expect(screen.queryByText('切换分支')).not.toBeInTheDocument();

    fireEvent.click(await screen.findByTestId('git-branch-create-toggle'));
    const input = screen.getByTestId('git-branch-create-input');
    expect(screen.getByTestId('git-branch-create-submit')).toBeDisabled();
    fireEvent.change(input, { target: { value: 'feat/brand-new' } });
    fireEvent.click(screen.getByTestId('git-branch-create-submit'));

    await waitFor(() => expect(gitCreateBranchMock).toHaveBeenCalledWith('/ws/linked', 'feat/brand-new', undefined, null));
    await waitFor(() => expect(fetchGitStatusMock.mock.calls.length).toBeGreaterThanOrEqual(2));
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已创建并切换到 feat/brand-new', 'success');
  });

  it('surfaces branch creation failures instead of silently swallowing them', async () => {
    gitCreateBranchMock.mockResolvedValue({ httpOk: false, code: 'already_exists', error: 'already_exists' });
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-branch-row')).toHaveTextContent('feat/knowledge-retrieval-research'));

    fireEvent.click(screen.getByTestId('git-env-branch-row'));
    fireEvent.click(await screen.findByTestId('git-branch-create-toggle'));
    fireEvent.change(screen.getByTestId('git-branch-create-input'), { target: { value: 'main' } });
    fireEvent.click(screen.getByTestId('git-branch-create-submit'));

    await waitFor(() => expect(useStore.getState().addToast).toHaveBeenCalledWith('分支已存在', 'error'));
  });

  it('creates a worktree and opens a new session inside it', async () => {
    gitCreateWorktreeMock.mockResolvedValue({
      httpOk: true,
      ok: true,
      path: '/repo/worktrees/fix-login-race',
      branch: 'wt/fix-login-race',
    });
    createLocalStudioWorkspaceFromFolderMock.mockResolvedValue({
      mountId: 'ws-fix-login-race',
      label: 'fix-login-race',
      nativeRootPath: '/repo/worktrees/fix-login-race',
    });

    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-worktree-row')).toBeEnabled());

    fireEvent.click(screen.getByTestId('git-env-worktree-row'));
    const nameInput = await screen.findByTestId('git-worktree-name');
    // 描述里内嵌落地根，基线默认当前分支
    expect(screen.getByTestId('git-worktree-desc')).toHaveTextContent('/repo/worktrees');
    expect((screen.getByTestId('git-worktree-base') as HTMLSelectElement).value).toBe('feat/knowledge-retrieval-research');
    expect(screen.getByTestId('git-worktree-create')).toBeDisabled();

    fireEvent.change(nameInput, { target: { value: 'fix-login-race' } });
    expect(screen.getByTestId('git-worktree-create')).toBeEnabled();
    fireEvent.click(screen.getByTestId('git-worktree-create'));

    await waitFor(() => expect(gitCreateWorktreeMock).toHaveBeenCalledWith('/ws/linked', {
      name: 'fix-login-race',
      base: 'feat/knowledge-retrieval-research',
      agentId: null,
    }));
    // 建成后把该目录注册成工作台并切过去（开新会话），当前检出不被改动
    await waitFor(() => expect(createLocalStudioWorkspaceFromFolderMock).toHaveBeenCalledWith('/repo/worktrees/fix-login-race'));
    await waitFor(() => expect(applyStudioWorkspaceMock).toHaveBeenCalled());
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已在 wt/fix-login-race 中打开新会话', 'success');
    expect(gitCheckoutMock).not.toHaveBeenCalled();
  });

  it('blocks invalid worktree names before hitting the server', async () => {
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-worktree-row')).toBeEnabled());

    fireEvent.click(screen.getByTestId('git-env-worktree-row'));
    const nameInput = await screen.findByTestId('git-worktree-name');
    fireEvent.change(nameInput, { target: { value: '../escape' } });

    expect(screen.getByTestId('git-worktree-create')).toBeDisabled();
    expect(screen.getByTestId('git-worktree-name-hint')).toHaveTextContent('名称不合法');
    expect(gitCreateWorktreeMock).not.toHaveBeenCalled();
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

  it('collapses and expands the rows from the title bar', async () => {
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-changes-row')).toHaveTextContent('+73,390'));

    const toggle = screen.getByRole('button', { name: /环境信息/ });
    fireEvent.click(toggle);
    // Collapse 走 motion 退场动画，卸载有延迟
    await waitFor(() => expect(screen.queryByTestId('git-env-changes-row')).not.toBeInTheDocument());
    expect(screen.queryByTestId('git-env-history-row')).not.toBeInTheDocument();

    fireEvent.click(toggle);
    await waitFor(() => expect(screen.getByTestId('git-env-changes-row')).toBeInTheDocument());
    expect(screen.getByTestId('git-env-history-row')).toBeInTheDocument();
  });

  it('opens the commit history modal from the history row', async () => {
    render(<GitEnvironmentCard />);
    await waitFor(() => expect(screen.getByTestId('git-env-changes-row')).toHaveTextContent('+73,390'));

    fireEvent.click(screen.getByTestId('git-env-history-row'));
    expect(await screen.findByText('feat: 头号提交')).toBeInTheDocument();
    expect(fetchGitLogMock).toHaveBeenCalledWith('/ws/linked', null, 300);
  });
});
