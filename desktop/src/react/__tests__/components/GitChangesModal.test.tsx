// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { GitChangesModal } from '../../components/runtime/GitChangesModal';
import type { GitFileChange, GitFileDiff, GitStashEntry } from '../../utils/git-env-api';

const fetchGitFileDiffMock = vi.fn<(dir: string, file: string) => Promise<GitFileDiff>>();
const fetchGitStashesMock = vi.fn();
const gitStashMock = vi.fn();
const gitUnstashMock = vi.fn();
const gitDiscardMock = vi.fn();

vi.mock('../../utils/git-env-api', () => ({
  fetchGitFileDiff: (dir: string, file: string) => fetchGitFileDiffMock(dir, file),
  fetchGitStashes: (...args: unknown[]) => fetchGitStashesMock(...args),
  gitStash: (...args: unknown[]) => gitStashMock(...args),
  gitUnstash: (...args: unknown[]) => gitUnstashMock(...args),
  gitDiscard: (...args: unknown[]) => gitDiscardMock(...args),
}));

const FILES: GitFileChange[] = [
  { path: 'desktop/src/react/components/chat/VeryLongComponentNameThatWillTruncate.tsx', additions: 4273, deletions: 12, state: 'modified', staged: false },
  { path: 'server/git/git-command.ts', additions: 100, deletions: 0, state: 'added', staged: true },
  { path: 'binary.png', additions: 0, deletions: 0, state: 'binary', staged: false },
];

/** 未跟踪文件：不提供回退，也不在储藏里 */
const UNTRACKED: GitFileChange = {
  path: 'scratch/new-file.ts', additions: 3, deletions: 0, state: 'untracked', staged: false,
};

const STASHES: GitStashEntry[] = [
  { ref: 'stash@{0}', message: 'On main: wip git-command', tracked: ['server/git/git-command.ts'], untracked: [] },
];

const TABLE: Record<string, string> = {
  'common.cancel': '取消',
  'gitEnv.changesTitle': '变更文件',
  'gitEnv.noChanges': '暂无变更',
  'gitEnv.noChangesWithStash': '工作区没有未提交的改动；储藏里还有 {n} 条，可点「取出全部」取回。',
  'gitEnv.diffBinary': '二进制文件，不支持查看 diff',
  'gitEnv.diffUnavailable': '无法读取 diff',
  'gitEnv.diffTruncated': '内容过长，已截断显示',
  'gitEnv.stashOne': '暂存',
  'gitEnv.unstashOne': '取出',
  'gitEnv.discardOne': '回退',
  'gitEnv.stashAll': '暂存全部',
  'gitEnv.unstashAll': '取出全部',
  'gitEnv.discardAll': '回退全部',
  'gitEnv.stashAllLabel': '暂存全部改动',
  'gitEnv.stashCount': '储藏 {n} 条',
  'gitEnv.stashFileDone': '已暂存 {name}',
  'gitEnv.stashFileFailed': '暂存失败',
  'gitEnv.stashAllDone': '已暂存全部改动',
  'gitEnv.unstashFileDone': '已取出 {name}',
  'gitEnv.unstashFileFailed': '取出失败',
  'gitEnv.unstashAllDone': '已取出最新一条储藏',
  'gitEnv.unstashNoStash': '没有储藏条目',
  'gitEnv.unstashNotInStash': '该文件不在任何储藏里',
  'gitEnv.unstashPathDirty': '该文件当前有未提交的改动，取出会覆盖它',
  'gitEnv.unstashConflict': '取出时发生冲突',
  'gitEnv.discardOneHint': '丢弃该文件未提交的改动',
  'gitEnv.discardUntrackedHint': '未跟踪的新文件不参与回退',
  'gitEnv.discardFileDone': '已回退 {name}',
  'gitEnv.discardAllDone': '已回退全部已跟踪改动',
  'gitEnv.discardFailed': '回退失败',
  'gitEnv.discardNothing': '没有可回退的改动',
  'gitEnv.discardConfirmTitle': '回退修改',
  'gitEnv.discardConfirmOk': '确认回退',
  'gitEnv.discardConfirmAll': '将丢弃全部已跟踪文件的未提交改动，无法撤销。',
  'gitEnv.discardConfirmFiles': '将丢弃这 {count} 个文件的未提交改动，无法撤销：\n{list}',
};

function makeT() {
  return ((key: string, vars?: Record<string, string | number>) => {
    const template = TABLE[key] ?? key;
    return template.replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
  }) as typeof window.t;
}

function renderModal(files: GitFileChange[] = FILES) {
  const refresh = vi.fn().mockResolvedValue(null);
  render(
    <GitChangesModal
      open
      onClose={vi.fn()}
      dir="/ws"
      files={files}
      agentId="hana"
      refresh={refresh}
    />,
  );
  return { refresh };
}

describe('GitChangesModal', () => {
  beforeEach(() => {
    window.t = makeT();
    fetchGitFileDiffMock.mockReset();
    fetchGitStashesMock.mockReset().mockResolvedValue({ isRepo: true, stashes: STASHES });
    gitStashMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    gitUnstashMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    gitDiscardMock.mockReset().mockResolvedValue({ httpOk: true, ok: true });
    useStore.setState({ addToast: vi.fn() } as never);
  });

  afterEach(() => cleanup());

  it('lists every changed file with per-file stats', () => {
    renderModal();
    expect(screen.getByText('变更文件')).toBeInTheDocument();
    expect(screen.getByTestId('git-change-desktop/src/react/components/chat/VeryLongComponentNameThatWillTruncate.tsx')).toHaveTextContent('+4,273');
    expect(screen.getByTestId('git-change-server/git/git-command.ts')).toHaveTextContent('-0');
  });

  it('shows the empty state when there are no changes', () => {
    renderModal([]);
    expect(screen.getByText('暂无变更')).toBeInTheDocument();
  });

  it('keeps take-back reachable after everything was stashed', async () => {
    // 暂存全部之后文件列表会清空：工具条与「取出全部」必须还在，否则改动就取不回来了
    renderModal([]);

    const empty = await screen.findByTestId('git-changes-empty');
    expect(empty).toHaveTextContent('储藏里还有 1 条');
    expect(screen.getByTestId('git-unstash-all')).toBeEnabled();
    // 没有文件可暂存 / 可回退，这两个按钮置灰但仍在
    expect(screen.getByTestId('git-stash-all')).toBeDisabled();
    expect(screen.getByTestId('git-discard-all')).toBeDisabled();
    expect(screen.getByTestId('git-stash-count')).toHaveTextContent('储藏 1 条');

    fireEvent.click(screen.getByTestId('git-unstash-all'));
    await waitFor(() => expect(gitUnstashMock).toHaveBeenCalledWith('/ws', { agentId: 'hana' }));
  });

  it('reports an empty working tree without stashes plainly', async () => {
    fetchGitStashesMock.mockResolvedValue({ isRepo: true, stashes: [] });
    renderModal([]);

    expect(await screen.findByTestId('git-changes-empty')).toHaveTextContent('暂无变更');
    expect(screen.getByTestId('git-unstash-all')).toBeDisabled();
  });

  it('expands a file row into an inline colored diff on click', async () => {
    fetchGitFileDiffMock.mockResolvedValue({
      path: 'server/git/git-command.ts',
      binary: false,
      patch: [
        'diff --git a/server/git/git-command.ts b/server/git/git-command.ts',
        'index 111..222 100644',
        '--- a/server/git/git-command.ts',
        '+++ b/server/git/git-command.ts',
        '@@ -1,2 +1,3 @@',
        ' import { Hono } from "hono";',
        '-const old = 1;',
        '+const neu = 2;',
        '+const added = 3;',
      ].join('\n'),
    });
    renderModal();

    fireEvent.click(screen.getByTestId('git-change-server/git/git-command.ts'));

    const pane = await screen.findByTestId('git-diff-server/git/git-command.ts');
    expect(pane).toHaveTextContent('const neu = 2;');
    expect(pane).toHaveTextContent('const old = 1;');
    expect(fetchGitFileDiffMock).toHaveBeenCalledWith('/ws', 'server/git/git-command.ts');

    // 再点一次收起
    fireEvent.click(screen.getByTestId('git-change-server/git/git-command.ts'));
    expect(screen.queryByTestId('git-diff-server/git/git-command.ts')).not.toBeInTheDocument();
  });

  it('reports binary files instead of a diff', async () => {
    fetchGitFileDiffMock.mockResolvedValue({ path: 'binary.png', patch: '', binary: true });
    renderModal();

    fireEvent.click(screen.getByTestId('git-change-binary.png'));
    expect(await screen.findByText('二进制文件，不支持查看 diff')).toBeInTheDocument();
  });

  it('caches diffs: a second expansion does not refetch', async () => {
    fetchGitFileDiffMock.mockResolvedValue({ path: 'server/git/git-command.ts', patch: '@@ -0,0 +1 @@\n+x\n', binary: false });
    renderModal();

    fireEvent.click(screen.getByTestId('git-change-server/git/git-command.ts'));
    await screen.findByTestId('git-diff-server/git/git-command.ts');
    fireEvent.click(screen.getByTestId('git-change-server/git/git-command.ts'));
    fireEvent.click(screen.getByTestId('git-change-server/git/git-command.ts'));
    await screen.findByTestId('git-diff-server/git/git-command.ts');
    expect(fetchGitFileDiffMock).toHaveBeenCalledTimes(1);
  });

  it('shows an error note when diff fetch fails', async () => {
    fetchGitFileDiffMock.mockRejectedValue(new Error('boom'));
    renderModal();

    fireEvent.click(screen.getByTestId('git-change-server/git/git-command.ts'));
    expect(await screen.findByText('无法读取 diff')).toBeInTheDocument();
  });

  // ── 文件级：暂存 / 取出 / 回退 ────────────────────────────────────────────

  it('offers the three per-file actions and gates take-back by the stash stack', async () => {
    renderModal([...FILES, UNTRACKED]);

    expect(await screen.findByTestId('git-stash-count')).toHaveTextContent('储藏 1 条');
    expect(screen.getByTestId('git-stash-all')).toBeEnabled();
    expect(screen.getByTestId('git-unstash-all')).toBeEnabled();
    expect(screen.getByTestId('git-discard-all')).toBeEnabled();
    expect(fetchGitStashesMock).toHaveBeenCalledWith('/ws', 'hana');

    // 在储藏里 → 可取；未跟踪的新文件不在储藏里，也不提供回退
    expect(screen.getByTestId('git-change-stash-server/git/git-command.ts')).toBeEnabled();
    expect(screen.getByTestId('git-change-unstash-server/git/git-command.ts')).toBeEnabled();
    expect(screen.getByTestId('git-change-discard-server/git/git-command.ts')).toBeEnabled();
    expect(screen.getByTestId('git-change-unstash-scratch/new-file.ts')).toBeDisabled();
    expect(screen.getByTestId('git-change-discard-scratch/new-file.ts')).toBeDisabled();
  });

  it('stashes one file by path and refreshes the card', async () => {
    const { refresh } = renderModal();
    await screen.findByTestId('git-stash-count');

    fireEvent.click(screen.getByTestId('git-change-stash-server/git/git-command.ts'));

    await waitFor(() => expect(gitStashMock).toHaveBeenCalledWith('/ws', {
      paths: ['server/git/git-command.ts'],
      agentId: 'hana',
    }));
    await waitFor(() => expect(refresh).toHaveBeenCalled());
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已暂存 server/git/git-command.ts', 'success');
  });

  it('takes one file back out of the stash by path', async () => {
    const { refresh } = renderModal();
    await screen.findByTestId('git-stash-count');

    fireEvent.click(screen.getByTestId('git-change-unstash-server/git/git-command.ts'));

    await waitFor(() => expect(gitUnstashMock).toHaveBeenCalledWith('/ws', {
      path: 'server/git/git-command.ts',
      agentId: 'hana',
    }));
    await waitFor(() => expect(refresh).toHaveBeenCalled());
    expect(useStore.getState().addToast).toHaveBeenCalledWith('已取出 server/git/git-command.ts', 'success');
  });

  it('confirms before discarding one file, then discards just that path', async () => {
    const { refresh } = renderModal();
    await screen.findByTestId('git-stash-count');

    fireEvent.click(screen.getByTestId('git-change-discard-server/git/git-command.ts'));
    expect(await screen.findByTestId('git-discard-confirm-body')).toHaveTextContent('将丢弃这 1 个文件');
    expect(gitDiscardMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText('确认回退'));
    await waitFor(() => expect(gitDiscardMock).toHaveBeenCalledWith('/ws', {
      paths: ['server/git/git-command.ts'],
      agentId: 'hana',
    }));
    await waitFor(() => expect(refresh).toHaveBeenCalled());
  });

  it('confirms then discards every tracked change at once', async () => {
    renderModal();
    await screen.findByTestId('git-stash-count');

    fireEvent.click(screen.getByTestId('git-discard-all'));
    expect(await screen.findByTestId('git-discard-confirm-body')).toHaveTextContent('全部已跟踪文件');

    fireEvent.click(screen.getByText('确认回退'));
    // 不带 paths = 整个仓库（未跟踪文件由服务端排除）
    await waitFor(() => expect(gitDiscardMock).toHaveBeenCalledWith('/ws', { agentId: 'hana' }));
  });

  it('stashes and takes back the whole tree from the toolbar', async () => {
    renderModal();
    await screen.findByTestId('git-stash-count');

    fireEvent.click(screen.getByTestId('git-stash-all'));
    await waitFor(() => expect(gitStashMock).toHaveBeenCalledWith('/ws', {
      message: '暂存全部改动',
      agentId: 'hana',
    }));

    fireEvent.click(screen.getByTestId('git-unstash-all'));
    await waitFor(() => expect(gitUnstashMock).toHaveBeenCalledWith('/ws', { agentId: 'hana' }));
  });

  it('disables take-back everywhere when the stash stack is empty', async () => {
    fetchGitStashesMock.mockResolvedValue({ isRepo: true, stashes: [] });
    renderModal();

    await waitFor(() => expect(screen.getByTestId('git-unstash-all')).toBeDisabled());
    expect(screen.getByTestId('git-change-unstash-server/git/git-command.ts')).toBeDisabled();
    expect(screen.getByTestId('git-stash-count')).toHaveTextContent('储藏 0 条');
  });

  it('surfaces structured failures instead of pretending success', async () => {
    gitUnstashMock.mockResolvedValue({ httpOk: false, code: 'path_dirty' });
    renderModal();
    await screen.findByTestId('git-stash-count');

    fireEvent.click(screen.getByTestId('git-change-unstash-server/git/git-command.ts'));

    await waitFor(() => expect(useStore.getState().addToast).toHaveBeenCalledWith(
      '该文件当前有未提交的改动，取出会覆盖它', 'error',
    ));
  });
});
