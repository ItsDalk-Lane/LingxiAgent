// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { GitGraphPanel } from '../../components/runtime/GitGraphPanel';
import type { GitBranches, GitFileDiff, GitStatus } from '../../utils/git-env-api';

// 面板行为聚焦：git 数据链路全部 mock 掉
const fetchGitFileDiffMock = vi.fn<(dir: string, file: string) => Promise<GitFileDiff>>();
const generateMock = vi.fn();
const gitCommitMock = vi.fn();
const gitAmendMock = vi.fn();
const gitStageMock = vi.fn();
const gitUnstageMock = vi.fn();
const gitPushMock = vi.fn();
const gitPullMock = vi.fn();
const gitFetchRemoteMock = vi.fn();
const gitDiscardMock = vi.fn();

vi.mock('../../utils/git-env-api', () => ({
  fetchGitStatus: vi.fn(),
  fetchGitBranches: vi.fn(),
  fetchGitWorktreeInfo: vi.fn(),
  fetchGitWorktrees: vi.fn(),
  fetchGitStashes: vi.fn(),
  fetchGitFileDiff: (dir: string, file: string) => fetchGitFileDiffMock(dir, file),
  fetchGitLog: vi.fn(),
  gitCheckout: vi.fn(),
  gitCreateBranch: vi.fn(),
  gitCreateWorktree: vi.fn(),
  gitCommit: (...args: unknown[]) => gitCommitMock(...args),
  gitAmend: (...args: unknown[]) => gitAmendMock(...args),
  gitStage: (...args: unknown[]) => gitStageMock(...args),
  gitUnstage: (...args: unknown[]) => gitUnstageMock(...args),
  gitPush: (...args: unknown[]) => gitPushMock(...args),
  gitPull: (...args: unknown[]) => gitPullMock(...args),
  gitFetchRemote: (...args: unknown[]) => gitFetchRemoteMock(...args),
  gitDiscard: (...args: unknown[]) => gitDiscardMock(...args),
  gitStash: vi.fn(),
  gitUnstash: vi.fn(),
  generateGitCommitMessage: (...args: unknown[]) => generateMock(...args),
}));

const BRANCHES: GitBranches = {
  isRepo: true,
  detached: false,
  current: 'feat/demo',
  branches: [{ name: 'feat/demo', current: true, checkedOutElsewhere: false }],
};

function makeStatus(overrides: Partial<GitStatus> = {}): GitStatus {
  return {
    isRepo: true,
    currentBranch: 'feat/demo',
    detached: false,
    total: { additions: 30, deletions: 4 },
    stagedTotal: { additions: 100, deletions: 0 },
    unstagedTotal: { additions: 3, deletions: 1 },
    files: [
      { path: 'server/index.ts', additions: 100, deletions: 0, state: 'added', staged: true },
      { path: 'desktop/src/app.tsx', additions: 27, deletions: 3, state: 'modified', staged: false },
      { path: 'lib/util.ts', additions: 0, deletions: 1, state: 'deleted', staged: false },
      { path: 'scratch/new-file.ts', additions: 3, deletions: 0, state: 'untracked', staged: false },
    ],
    hasUpstream: true,
    hasRemote: true,
    ahead: 0,
    behind: 0,
    commitable: true,
    pushable: true,
    ...overrides,
  };
}

const TABLE: Record<string, string> = {
  'common.cancel': '取消',
  'gitEnv.graphTitle': '源代码管理',
  'gitEnv.graphMessagePlaceholder': '消息(*Enter 在“{branch}”提交)',
  'gitEnv.graphCleanTree': '没有更改，工作区是干净的',
  'gitEnv.stagedChanges': '暂存的更改',
  'gitEnv.changes': '变更',
  'gitEnv.btnCommit': '提交',
  'gitEnv.btnCommitPush': '提交并推送',
  'gitEnv.amendBtn': '提交（修改）',
  'gitEnv.amendHint': '把改动并入上一次提交',
  'gitEnv.commitSyncBtn': '提交和同步',
  'gitEnv.commitSyncHint': '先拉远程，再提交并推送',
  'gitEnv.syncChanges': '同步更改',
  'gitEnv.syncPushHint': '推送本地的 {n} 个提交',
  'gitEnv.syncPullHint': '拉取远程的 {n} 个新提交',
  'gitEnv.syncBothHint': '先拉取远程的 {n} 个新提交，再推送本地的 {m} 个提交',
  'gitEnv.unstageAll': '取消所有暂存修改',
  'gitEnv.stageAllTitle': '暂存所有更改',
  'gitEnv.discardAllTitle': '放弃所有更改',
  'gitEnv.stageOne': '暂存更改',
  'gitEnv.unstageOne': '取消暂存修改',
  'gitEnv.discardOneTitle': '放弃更改',
  'gitEnv.discardUntrackedHint': '未跟踪的新文件不参与回退，避免误删',
  'gitEnv.discardConfirmTitle': '回退修改',
  'gitEnv.discardConfirmOk': '确认回退',
  'gitEnv.discardConfirmAll': '将丢弃全部已跟踪文件的未提交改动，无法撤销。',
  'gitEnv.discardConfirmFiles': '将丢弃这 {count} 个文件的未提交改动，无法撤销：\n{list}',
  'gitEnv.amendDone': '已并入上一次提交',
  'gitEnv.stageDone': '已暂存 {name}',
  'gitEnv.unstageDone': '已取消暂存 {name}',
  'gitEnv.commitDone': '提交完成',
  'gitEnv.pushDone': '推送完成',
  'gitEnv.pullDone': '已拉取远程更新（{count} 个提交）',
  'gitEnv.pullUpToDate': '已是最新，无需拉取',
  'gitEnv.aiGenerating': '正在生成提交信息…',
  'gitEnv.aiGeneratingShort': '生成中…',
  'gitEnv.aiFailed': '提交信息生成失败',
  'gitEnv.operationFailed': '操作失败',
  'gitEnv.checkBlockEof': '提交被项目的自动检查拦下了：文件末尾有多余的空行（{file} 第 {line} 行），删掉它再提交。',
  'gitEnv.checkBlockPush': '推送被拒绝了：常见原因是①推送前的自动检查（如整个项目的类型检查）没有通过，②远程分支有你本地没有的新提交。可先拉取一次再看，或在本机终端查看检查输出。',
  'gitEnv.refreshInfo': '刷新远程与本地信息',
  'gitEnv.refreshDone': '已刷新：本地与远程信息都是最新的',
  'gitEnv.noRemoteNotice': '没有添加远程仓库——提交只保存在本地，无法推送。',
  'gitEnv.nothingStaged': '没有已暂存的更改',
  'gitEnv.nothingToCommit': '没有可提交的更改',
  'gitEnv.nothingToPush': '没有可推送的提交',
  'gitEnv.branchesTitle': '切换分支',
  'gitEnv.diffBinary': '二进制文件，不支持查看 diff',
  'gitEnv.diffUnavailable': '无法读取 diff',
  'gitEnv.diffTruncated': '内容过长，已截断显示',
  'gitEnv.letterM': '修改',
  'gitEnv.letterU': '新文件',
  'gitEnv.letterA': '已暂存的新文件',
  'gitEnv.letterD': '已删除',
};

function makeT() {
  return ((key: string, vars?: Record<string, string | number>) => {
    const template = TABLE[key] ?? key;
    return template.replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
  }) as typeof window.t;
}

function renderPanel(status: GitStatus = makeStatus()) {
  const refresh = vi.fn().mockResolvedValue(status);
  render(
    <GitGraphPanel
      open
      onClose={vi.fn()}
      dir="/ws"
      status={status}
      branches={BRANCHES}
      sessionPath={null}
      agentId="hana"
      refresh={refresh}
    />,
  );
  return { refresh };
}

describe('GitGraphPanel', () => {
  beforeEach(() => {
    window.t = makeT();
    fetchGitFileDiffMock.mockReset().mockResolvedValue({ path: 'x', patch: '', binary: false });
    generateMock.mockReset().mockResolvedValue({ httpOk: true, message: 'feat: 模型生成的信息' });
    gitCommitMock.mockReset().mockResolvedValue({ httpOk: true, ok: true, head: 'a'.repeat(40) });
    gitAmendMock.mockReset().mockResolvedValue({ httpOk: true, ok: true, head: 'b'.repeat(40) });
    gitStageMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    gitUnstageMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    gitPushMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    gitPullMock.mockReset().mockResolvedValue({ httpOk: true, ok: true, pulled: 2 });
    gitFetchRemoteMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    gitDiscardMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    useStore.setState({ addToast: vi.fn() } as never);
  });

  afterEach(() => cleanup());

  it('splits files into staged and changes sections with count badges', () => {
    renderPanel();
    expect(screen.getByText('源代码管理')).toBeInTheDocument();
    expect(screen.getByTestId('git-graph-staged-head')).toHaveTextContent('暂存的更改');
    expect(screen.getByTestId('git-graph-staged-head')).toHaveTextContent('1');
    expect(screen.getByTestId('git-graph-changes-head')).toHaveTextContent('变更');
    expect(screen.getByTestId('git-graph-changes-head')).toHaveTextContent('3');
    // 分区工具：暂存区只有取消所有暂存修改；变更区是放弃所有更改 + 暂存所有更改
    expect(screen.getByTestId('git-unstage-all')).toBeInTheDocument();
    expect(screen.getByTestId('git-discard-all')).toBeInTheDocument();
    expect(screen.getByTestId('git-stage-all')).toBeInTheDocument();
  });

  it('keeps row actions pinned next to the status letter; untracked rows discard too', () => {
    renderPanel();
    // 未跟踪文件也可放弃（= 删除该文件），不再置灰
    expect(screen.getByTestId('git-graph-row-discard-scratch/new-file.ts')).toBeEnabled();
    expect(screen.getByTestId('git-graph-row-stage-scratch/new-file.ts')).toBeEnabled();
    // 暂存区行内只有取消暂存修改
    expect(screen.getByTestId('git-graph-row-unstage-server/index.ts')).toBeInTheDocument();
    expect(screen.queryByTestId('git-graph-row-stage-server/index.ts')).not.toBeInTheDocument();
  });

  it('stages and unstages individual files through the row actions', async () => {
    const { refresh } = renderPanel();

    fireEvent.click(screen.getByTestId('git-graph-row-stage-desktop/src/app.tsx'));
    await waitFor(() => expect(gitStageMock).toHaveBeenCalledWith('/ws', { paths: ['desktop/src/app.tsx'], agentId: 'hana' }));
    await waitFor(() => expect(refresh).toHaveBeenCalled());

    fireEvent.click(screen.getByTestId('git-graph-row-unstage-server/index.ts'));
    await waitFor(() => expect(gitUnstageMock).toHaveBeenCalledWith('/ws', { paths: ['server/index.ts'], agentId: 'hana' }));
  });

  it('discards a file through the confirm dialog', async () => {
    renderPanel();

    fireEvent.click(screen.getByTestId('git-graph-row-discard-desktop/src/app.tsx'));
    expect(await screen.findByTestId('git-discard-confirm-body')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '确认回退' }));

    await waitFor(() => expect(gitDiscardMock).toHaveBeenCalledWith('/ws', { paths: ['desktop/src/app.tsx'], agentId: 'hana' }));
  });

  it('discards every tracked change through the section tool after confirmation', async () => {
    renderPanel();

    fireEvent.click(screen.getByTestId('git-discard-all'));
    fireEvent.click(await screen.findByRole('button', { name: '确认回退' }));

    await waitFor(() => expect(gitDiscardMock).toHaveBeenCalledWith('/ws', { agentId: 'hana' }));
  });

  it('commits staged-only changes and refreshes without closing', async () => {
    const { refresh } = renderPanel();

    fireEvent.change(screen.getByTestId('git-graph-message'), { target: { value: 'feat: 手写信息' } });
    fireEvent.click(screen.getByTestId('git-graph-main-btn'));

    await waitFor(() => expect(gitCommitMock).toHaveBeenCalledWith('/ws', {
      message: 'feat: 手写信息', includeUnstaged: false, agentId: 'hana',
    }));
    expect(generateMock).not.toHaveBeenCalled();
    await waitFor(() => expect(refresh).toHaveBeenCalled());
    // 提交成功后输入框清空，避免"看起来没提交成功"
    expect(screen.getByTestId('git-graph-message')).toHaveValue('');
    // 面板保持打开
    expect(screen.getByTestId('git-graph-sections')).toBeInTheDocument();
  });

  it('translates project gate failures into plain language instead of raw git stderr', async () => {
    gitCommitMock.mockResolvedValue({
      httpOk: false,
      ok: false,
      error: 'lefthook ran\nBENCHMARK.md:4: new blank line at EOF\nexit status 2',
    });
    renderPanel();

    fireEvent.change(screen.getByTestId('git-graph-message'), { target: { value: 'msg' } });
    fireEvent.click(screen.getByTestId('git-graph-main-btn'));

    await waitFor(() => expect(useStore.getState().addToast).toHaveBeenCalledWith(
      expect.stringContaining('文件末尾有多余的空行（BENCHMARK.md 第 4 行）'),
      'error',
    ));
    // 原始报错不再直接甩给用户
    expect(useStore.getState().addToast).not.toHaveBeenCalledWith(
      expect.stringContaining('new blank line at EOF'),
      'error',
    );
  });

  it('translates pre-push gate failures during sync', async () => {
    gitPushMock.mockResolvedValue({
      httpOk: false,
      ok: false,
      code: 'push_failed',
      message: "error: failed to push some refs to 'https://example.test/repo.git'",
    });
    renderPanel(makeStatus({ ahead: 1, files: [] }));

    fireEvent.click(screen.getByTestId('git-graph-main-btn'));
    await waitFor(() => expect(useStore.getState().addToast).toHaveBeenCalledWith(
      expect.stringContaining('推送被拒绝了'),
      'error',
    ));
    expect(useStore.getState().addToast).not.toHaveBeenCalledWith(
      expect.stringContaining('failed to push some refs'),
      'error',
    );
  });

  it('auto-stages everything when committing with an empty index', async () => {
    renderPanel(makeStatus({ files: [makeStatus().files[1]!] }));

    fireEvent.click(screen.getByTestId('git-graph-main-btn'));

    // 信息留空 → 模型生成（上下文含未暂存），随后提交全部改动
    await waitFor(() => expect(generateMock).toHaveBeenCalledWith('/ws', {
      includeUnstaged: true, sessionPath: null, agentId: 'hana',
    }));
    await waitFor(() => expect(gitCommitMock).toHaveBeenCalledWith('/ws', {
      message: 'feat: 模型生成的信息', includeUnstaged: true, agentId: 'hana',
    }));
  });

  it('amends from the menu, staging everything first when the index is empty', async () => {
    renderPanel(makeStatus({ files: [makeStatus().files[1]!] }));

    fireEvent.click(screen.getByTestId('git-graph-menu-btn'));
    fireEvent.click(await screen.findByTestId('git-graph-menu-amend'));

    await waitFor(() => expect(gitStageMock).toHaveBeenCalledWith('/ws', { agentId: 'hana' }));
    await waitFor(() => expect(gitAmendMock).toHaveBeenCalledWith('/ws', { message: null, agentId: 'hana' }));
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已并入上一次提交', 'success');
  });

  it('commits and pushes from the menu', async () => {
    renderPanel();

    fireEvent.click(screen.getByTestId('git-graph-menu-btn'));
    fireEvent.click(await screen.findByTestId('git-graph-menu-commit-push'));

    await waitFor(() => expect(gitCommitMock).toHaveBeenCalled());
    await waitFor(() => expect(gitPushMock).toHaveBeenCalledWith('/ws', 'hana'));
  });

  it('turns the main button into push when only ahead of the remote', async () => {
    renderPanel(makeStatus({ ahead: 6 }));

    const main = screen.getByTestId('git-graph-main-btn');
    expect(main).toHaveTextContent('同步更改');
    expect(main).toHaveTextContent('6');

    fireEvent.click(main);
    await waitFor(() => expect(gitPushMock).toHaveBeenCalledWith('/ws', 'hana'));
    expect(gitCommitMock).not.toHaveBeenCalled();
  });

  it('turns the main button into pull when only behind the remote', async () => {
    renderPanel(makeStatus({ behind: 2 }));

    fireEvent.click(screen.getByTestId('git-graph-main-btn'));
    await waitFor(() => expect(gitPullMock).toHaveBeenCalledWith('/ws', 'hana'));
    expect(gitPushMock).not.toHaveBeenCalled();
  });

  it('pulls first and pushes after when diverged', async () => {
    renderPanel(makeStatus({ ahead: 6, behind: 2 }));

    const main = screen.getByTestId('git-graph-main-btn');
    expect(main).toHaveTextContent('同步更改');

    fireEvent.click(main);
    await waitFor(() => expect(gitPullMock).toHaveBeenCalled());
    await waitFor(() => expect(gitPushMock).toHaveBeenCalled());
    expect(gitPullMock.mock.invocationCallOrder[0]).toBeLessThan(gitPushMock.mock.invocationCallOrder[0]);
  });

  it('shows the clean-tree empty state when there are no changes', () => {
    renderPanel(makeStatus({ files: [], commitable: false }));
    expect(screen.getByTestId('git-graph-clean')).toHaveTextContent('没有更改，工作区是干净的');
  });

  it('expands the file diff lazily on row click', async () => {
    fetchGitFileDiffMock.mockResolvedValue({
      path: 'desktop/src/app.tsx',
      patch: '@@ -1,2 +1,3 @@\n-old line\n+new line',
      binary: false,
    });
    renderPanel();

    fireEvent.click(screen.getByTestId('git-graph-file-toggle-desktop/src/app.tsx'));
    await waitFor(() => expect(fetchGitFileDiffMock).toHaveBeenCalledWith('/ws', 'desktop/src/app.tsx'));
    // 解析器剥掉 +/- 前缀后渲染
    expect(await screen.findByText('new line')).toBeInTheDocument();
    expect(screen.getByText('old line')).toBeInTheDocument();

    // 再点收起
    fireEvent.click(screen.getByTestId('git-graph-file-toggle-desktop/src/app.tsx'));
    expect(screen.queryByText('new line')).not.toBeInTheDocument();
  });

  it('collapses a section from anywhere on its header row, not just the chevron', () => {
    renderPanel();
    const head = screen.getByTestId('git-graph-changes-head');
    expect(head).toHaveAttribute('aria-expanded', 'true');

    // 点标题文字（旧行为只有折叠箭头本身可点）
    fireEvent.click(screen.getByText('变更'));
    expect(head).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByTestId('git-graph-file-toggle-desktop/src/app.tsx')).not.toBeInTheDocument();

    // 再点整行任意位置展开；行内工具按钮不触发折叠
    fireEvent.click(head);
    expect(screen.getByTestId('git-graph-file-toggle-desktop/src/app.tsx')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('git-stage-all'));
    expect(head).toHaveAttribute('aria-expanded', 'true');
  });

  it('opens the branch popover from the panel header chip', async () => {
    renderPanel();

    fireEvent.click(screen.getByTestId('git-graph-branch'));
    expect(await screen.findByTestId('git-graph-branch-feat/demo')).toBeInTheDocument();
  });

  it('shows the no-remote notice when the repository has no remote configured', () => {
    renderPanel(makeStatus({ hasRemote: false, hasUpstream: false, pushable: false }));
    expect(screen.getByTestId('git-graph-no-remote')).toHaveTextContent('没有添加远程仓库');
  });

  it('refreshes remote info from the header button', async () => {
    const { refresh } = renderPanel();

    fireEvent.click(screen.getByTestId('git-graph-refresh'));
    await waitFor(() => expect(gitFetchRemoteMock).toHaveBeenCalledWith('/ws', 'hana'));
    await waitFor(() => expect(refresh).toHaveBeenCalled());
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已刷新：本地与远程信息都是最新的', 'success');
  });
});
