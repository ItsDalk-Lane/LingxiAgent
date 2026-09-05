// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { GitCommitModal } from '../../components/runtime/GitCommitModal';
import type { GitBranches, GitStatus } from '../../utils/git-env-api';

const generateGitCommitMessageMock = vi.fn();
const gitCommitMock = vi.fn();
const gitPushMock = vi.fn();
const gitCheckoutMock = vi.fn();

vi.mock('../../utils/git-env-api', () => ({
  generateGitCommitMessage: (...args: unknown[]) => generateGitCommitMessageMock(...args),
  gitCommit: (...args: unknown[]) => gitCommitMock(...args),
  gitPush: (...args: unknown[]) => gitPushMock(...args),
  gitCheckout: (...args: unknown[]) => gitCheckoutMock(...args),
}));

function makeStatus(overrides: Partial<GitStatus> = {}): GitStatus {
  return {
    isRepo: true,
    currentBranch: 'feat/demo',
    detached: false,
    total: { additions: 4373, deletions: 0 },
    stagedTotal: { additions: 100, deletions: 0 },
    unstagedTotal: { additions: 4273, deletions: 2 },
    files: [
      { path: 'a.ts', additions: 4273, deletions: 2, state: 'modified', staged: false },
      { path: 'b.ts', additions: 100, deletions: 0, state: 'added', staged: true },
    ],
    hasUpstream: false,
    hasRemote: true,
    ahead: 0,
    behind: 0,
    commitable: true,
    pushable: true,
    ...overrides,
  };
}

const BRANCHES: GitBranches = {
  isRepo: true,
  detached: false,
  current: 'feat/demo',
  branches: [
    { name: 'feat/demo', current: true, checkedOutElsewhere: false },
    { name: 'main', current: false, checkedOutElsewhere: false },
  ],
};

const TABLE: Record<string, string> = {
  'gitEnv.commitTitle': '提交或推送',
  'gitEnv.commitMessagePlaceholder': '提交信息（留空将自动生成）',
  'gitEnv.includeUnstaged': '包含未暂存的更改',
  'gitEnv.btnCommit': '提交',
  'gitEnv.btnCommitPush': '提交并推送',
  'gitEnv.btnPush': '推送',
  'gitEnv.aiGenerating': '正在生成提交信息…',
  'gitEnv.aiFailed': '提交信息生成失败',
  'gitEnv.commitDone': '提交完成',
  'gitEnv.pushDone': '推送完成',
  'gitEnv.nothingStaged': '没有已暂存的更改',
  'gitEnv.nothingToCommit': '没有可提交的更改',
  'gitEnv.nothingToPush': '没有可推送的提交',
  'gitEnv.noRemote': '未配置远程仓库',
  'gitEnv.operationFailed': '操作失败',
  'gitEnv.branchesTitle': '切换分支',
  'gitEnv.detachedHead': '分离头指针（{name}）',
};

function makeT() {
  return ((key: string, vars?: Record<string, string | number>) => {
    const template = TABLE[key] ?? key;
    return template.replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
  }) as typeof window.t;
}

function renderModal(status: GitStatus, refresh = vi.fn().mockResolvedValue(status)) {
  const onClose = vi.fn();
  render(
    <GitCommitModal
      open
      onClose={onClose}
      dir="/ws"
      status={status}
      branches={BRANCHES}
      sessionPath="/sessions/s.jsonl"
      agentId="hana"
      refresh={refresh}
    />,
  );
  return { onClose, refresh };
}

describe('GitCommitModal', () => {
  beforeEach(() => {
    window.t = makeT();
    generateGitCommitMessageMock.mockReset();
    gitCommitMock.mockReset();
    gitPushMock.mockReset();
    gitCheckoutMock.mockReset();
    useStore.setState({ addToast: vi.fn() } as never);
  });

  afterEach(() => cleanup());

  it('shows branch, unstaged stats and three actions with full availability', () => {
    renderModal(makeStatus());
    expect(screen.getByTestId('git-commit-branch')).toHaveTextContent('feat/demo');
    expect(screen.getByTestId('git-commit-unstaged-stats')).toHaveTextContent('+4,273');
    expect(screen.getByTestId('git-commit-unstaged-stats')).toHaveTextContent('-2');
    expect(screen.getByTestId('git-commit-btn')).toBeEnabled();
    expect(screen.getByTestId('git-commit-push-btn')).toBeEnabled();
    expect(screen.getByTestId('git-push-btn')).toBeEnabled();
  });

  it('disables commit actions when nothing to commit and push when nothing to push', () => {
    renderModal(makeStatus({ commitable: false, files: [], pushable: false, hasRemote: false }));
    expect(screen.getByTestId('git-commit-btn')).toBeDisabled();
    expect(screen.getByTestId('git-commit-push-btn')).toBeDisabled();
    expect(screen.getByTestId('git-push-btn')).toBeDisabled();
  });

  it('keeps push-only available when tree is clean but commits await push', () => {
    renderModal(makeStatus({ commitable: false, files: [], hasUpstream: true, ahead: 2, pushable: true }));
    expect(screen.getByTestId('git-commit-btn')).toBeDisabled();
    expect(screen.getByTestId('git-commit-push-btn')).toBeEnabled();
    expect(screen.getByTestId('git-push-btn')).toBeEnabled();
  });

  it('uses the typed message without calling AI generation', async () => {
    gitCommitMock.mockResolvedValue({ httpOk: true, ok: true, head: 'abc1234' });
    const { onClose } = renderModal(makeStatus());

    fireEvent.change(screen.getByTestId('git-commit-message'), { target: { value: 'fix: 手动信息' } });
    fireEvent.click(screen.getByTestId('git-commit-btn'));

    await waitFor(() => expect(gitCommitMock).toHaveBeenCalledWith('/ws', {
      message: 'fix: 手动信息',
      includeUnstaged: true,
      agentId: 'hana',
    }));
    expect(generateGitCommitMessageMock).not.toHaveBeenCalled();
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(useStore.getState().addToast).toHaveBeenCalledWith('提交完成', 'success');
  });

  it('generates an AI message when left empty, backfills the textarea, then commits', async () => {
    generateGitCommitMessageMock.mockResolvedValue({ httpOk: true, message: 'feat: AI 生成' });
    gitCommitMock.mockResolvedValue({ httpOk: true, ok: true });
    const { onClose } = renderModal(makeStatus());

    fireEvent.click(screen.getByTestId('git-commit-btn'));

    await waitFor(() => expect(generateGitCommitMessageMock).toHaveBeenCalledWith('/ws', {
      includeUnstaged: true,
      sessionPath: '/sessions/s.jsonl',
      agentId: 'hana',
    }));
    await waitFor(() => expect(gitCommitMock).toHaveBeenCalledWith('/ws', {
      message: 'feat: AI 生成',
      includeUnstaged: true,
      agentId: 'hana',
    }));
    expect((screen.getByTestId('git-commit-message') as HTMLTextAreaElement).value).toBe('feat: AI 生成');
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it('aborts commit when AI generation fails', async () => {
    generateGitCommitMessageMock.mockResolvedValue({ httpOk: false, error: 'boom' });
    const { onClose } = renderModal(makeStatus());

    fireEvent.click(screen.getByTestId('git-commit-btn'));

    await waitFor(() => expect(useStore.getState().addToast).toHaveBeenCalledWith('boom', 'error'));
    expect(gitCommitMock).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('commit-and-push commits first, refreshes, then pushes', async () => {
    const status = makeStatus();
    generateGitCommitMessageMock.mockResolvedValue({ httpOk: true, message: 'feat: x' });
    gitCommitMock.mockResolvedValue({ httpOk: true, ok: true });
    gitPushMock.mockResolvedValue({ httpOk: true, ok: true });
    const refresh = vi.fn().mockResolvedValue({ ...status, commitable: false, pushable: true });
    const { onClose } = renderModal(status, refresh);

    fireEvent.click(screen.getByTestId('git-commit-push-btn'));

    await waitFor(() => expect(gitCommitMock).toHaveBeenCalled());
    await waitFor(() => expect(refresh).toHaveBeenCalled());
    await waitFor(() => expect(gitPushMock).toHaveBeenCalledWith('/ws', 'hana'));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(useStore.getState().addToast).toHaveBeenCalledWith('推送完成', 'success');
  });

  it('push surfaces structured failure as toast and stays open', async () => {
    gitPushMock.mockResolvedValue({ httpOk: false, code: 'no_remote' });
    const { onClose } = renderModal(makeStatus());

    fireEvent.click(screen.getByTestId('git-push-btn'));

    await waitFor(() => expect(useStore.getState().addToast).toHaveBeenCalledWith('未配置远程仓库', 'error'));
    expect(onClose).not.toHaveBeenCalled();
  });
});
