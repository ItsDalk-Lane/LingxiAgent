// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { GitChangesModal } from '../../components/runtime/GitChangesModal';
import type { GitFileChange, GitFileDiff } from '../../utils/git-env-api';

const fetchGitFileDiffMock = vi.fn<(dir: string, file: string) => Promise<GitFileDiff>>();

vi.mock('../../utils/git-env-api', () => ({
  fetchGitFileDiff: (dir: string, file: string) => fetchGitFileDiffMock(dir, file),
}));

const FILES: GitFileChange[] = [
  { path: 'desktop/src/react/components/chat/VeryLongComponentNameThatWillTruncate.tsx', additions: 4273, deletions: 12, state: 'modified', staged: false },
  { path: 'server/git/git-command.ts', additions: 100, deletions: 0, state: 'added', staged: true },
  { path: 'binary.png', additions: 0, deletions: 0, state: 'binary', staged: false },
];

const TABLE: Record<string, string> = {
  'gitEnv.changesTitle': '变更文件',
  'gitEnv.noChanges': '暂无变更',
  'gitEnv.diffBinary': '二进制文件，不支持查看 diff',
  'gitEnv.diffUnavailable': '无法读取 diff',
  'gitEnv.diffTruncated': '内容过长，已截断显示',
};

describe('GitChangesModal', () => {
  beforeEach(() => {
    window.t = (((key: string) => TABLE[key] ?? key) as typeof window.t);
    fetchGitFileDiffMock.mockReset();
  });

  afterEach(() => cleanup());

  it('lists every changed file with per-file stats', () => {
    render(<GitChangesModal open onClose={vi.fn()} dir="/ws" files={FILES} />);
    expect(screen.getByText('变更文件')).toBeInTheDocument();
    expect(screen.getByTestId('git-change-desktop/src/react/components/chat/VeryLongComponentNameThatWillTruncate.tsx')).toHaveTextContent('+4,273');
    expect(screen.getByTestId('git-change-server/git/git-command.ts')).toHaveTextContent('-0');
  });

  it('shows the empty state when there are no changes', () => {
    render(<GitChangesModal open onClose={vi.fn()} dir="/ws" files={[]} />);
    expect(screen.getByText('暂无变更')).toBeInTheDocument();
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
    render(<GitChangesModal open onClose={vi.fn()} dir="/ws" files={FILES} />);

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
    render(<GitChangesModal open onClose={vi.fn()} dir="/ws" files={FILES} />);

    fireEvent.click(screen.getByTestId('git-change-binary.png'));
    expect(await screen.findByText('二进制文件，不支持查看 diff')).toBeInTheDocument();
  });

  it('caches diffs: a second expansion does not refetch', async () => {
    fetchGitFileDiffMock.mockResolvedValue({ path: 'server/git/git-command.ts', patch: '@@ -0,0 +1 @@\n+x\n', binary: false });
    render(<GitChangesModal open onClose={vi.fn()} dir="/ws" files={FILES} />);

    fireEvent.click(screen.getByTestId('git-change-server/git/git-command.ts'));
    await screen.findByTestId('git-diff-server/git/git-command.ts');
    fireEvent.click(screen.getByTestId('git-change-server/git/git-command.ts'));
    fireEvent.click(screen.getByTestId('git-change-server/git/git-command.ts'));
    await screen.findByTestId('git-diff-server/git/git-command.ts');
    expect(fetchGitFileDiffMock).toHaveBeenCalledTimes(1);
  });

  it('shows an error note when diff fetch fails', async () => {
    fetchGitFileDiffMock.mockRejectedValue(new Error('boom'));
    render(<GitChangesModal open onClose={vi.fn()} dir="/ws" files={FILES} />);

    fireEvent.click(screen.getByTestId('git-change-server/git/git-command.ts'));
    expect(await screen.findByText('无法读取 diff')).toBeInTheDocument();
  });
});
