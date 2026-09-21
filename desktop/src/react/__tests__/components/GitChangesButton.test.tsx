// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { GitChangesButton } from '../../components/runtime/GitChangesButton';
import { resetGitEnvCache } from '../../hooks/use-git-env';
import type { GitBranches, GitStatus, GitWorktrees } from '../../utils/git-env-api';

// 入口行为聚焦：git 数据链路全部 mock 掉
const fetchGitStatusMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitStatus>>();
const fetchGitBranchesMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitBranches>>();
const fetchGitWorktreesMock = vi.fn<(dir: string, agentId?: string | null) => Promise<GitWorktrees>>();
const fetchGitStashesMock = vi.fn();
const gitCheckoutMock = vi.fn();
const gitCreateBranchMock = vi.fn();
const gitCreateWorktreeMock = vi.fn();
const fetchGitLogMock = vi.fn();

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
  ],
};

const TABLE: Record<string, string> = {
  'common.cancel': '取消',
  'common.close': '关闭',
  'gitEnv.title': '环境信息',
  'gitEnv.changes': '变更',
  'gitEnv.branch': '分支',
  'gitEnv.notGitRepo': '非 Git 仓库',
  'gitEnv.loadFailed': '加载失败，点击重试',
  'gitEnv.graphTitle': '源代码管理',
  'gitEnv.stagedChanges': '暂存的更改',
  'gitEnv.detachedHead': '分离头指针（{name}）',
};

function makeT() {
  return ((key: string, vars?: Record<string, string | number>) => {
    const template = TABLE[key] ?? key;
    return template.replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
  }) as typeof window.t;
}

describe('GitChangesButton', () => {
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
    fetchGitLogMock.mockReset().mockResolvedValue({ isRepo: true, commits: [] });
    useStore.setState({
      deskBasePath: '/repo/main',
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

  it('renders nothing when the workspace dir is not set', () => {
    useStore.setState({ deskBasePath: null, deskWorkspaceNativeRoot: null } as never);
    const { container } = render(<GitChangesButton />);
    expect(container.querySelector('[data-testid="desk-git-changes-btn"]')).not.toBeInTheDocument();
  });

  it('hides the button while probing and when the dir is not a git repo', async () => {
    let resolveStatus!: (v: GitStatus) => void;
    fetchGitStatusMock.mockReset().mockImplementation(() => new Promise<GitStatus>(resolve => { resolveStatus = resolve; }));

    const { container } = render(<GitChangesButton />);
    // 探测在途：不占工作台工具行位置
    expect(container.querySelector('[data-testid="desk-git-changes-btn"]')).not.toBeInTheDocument();

    resolveStatus({ ...STATUS, isRepo: false, files: [], commitable: false, pushable: false });
    await waitFor(() => expect(container.querySelector('[data-testid="desk-git-changes-btn"]')).not.toBeInTheDocument());
  });

  it('shows change totals with thousands separators once the repo is detected', async () => {
    render(<GitChangesButton />);
    const btn = await screen.findByTestId('desk-git-changes-btn');
    expect(btn).toHaveTextContent('变更');
    expect(btn).toHaveTextContent('+73,390');
    expect(btn).toHaveTextContent('-5,000');
  });

  it('keeps showing 变更 +0 -0 on a clean tree', async () => {
    fetchGitStatusMock.mockResolvedValue({ ...STATUS, total: { additions: 0, deletions: 0 }, files: [] });
    render(<GitChangesButton />);
    const btn = await screen.findByTestId('desk-git-changes-btn');
    expect(btn).toHaveTextContent('变更');
    expect(btn).toHaveTextContent('+0');
    expect(btn).toHaveTextContent('-0');
  });

  it('opens the source-control panel from the button', async () => {
    render(<GitChangesButton />);
    fireEvent.click(await screen.findByTestId('desk-git-changes-btn'));
    // 面板标题 + 暂存/变更两区（行名取路径末段）
    expect(await screen.findByRole('heading', { name: '源代码管理' })).toBeInTheDocument();
    expect(screen.getByTestId('git-graph-staged-head')).toHaveTextContent('暂存的更改');
    expect(screen.getByTestId('git-graph-file-server/index.ts')).toHaveTextContent('index.ts');
  });
});
