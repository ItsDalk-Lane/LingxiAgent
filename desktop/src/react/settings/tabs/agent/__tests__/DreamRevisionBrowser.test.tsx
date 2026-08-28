// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DreamRevisionBrowser } from '../DreamRevisionBrowser';
import {
  loadDreamRevision,
  loadDreamRevisions,
  restoreDream,
} from '../agent-memory-dream-actions';

vi.mock('../agent-memory-dream-actions', async (importOriginal) => {
  // 纯函数（dreamSectionsEqual 等）保留真实实现，只 mock 网络动作层。
  const actual = await importOriginal<typeof import('../agent-memory-dream-actions')>();
  return {
    ...actual,
    loadDreamRevision: vi.fn(),
    loadDreamRevisions: vi.fn(),
    restoreDream: vi.fn(),
  };
});

vi.mock('../../../helpers', () => ({
  t: (key: string) => ({
    'error.code.dreamRevisionNotFound': '找不到这个 Dream 版本，它可能已经被清理',
    'settings.memory.dream.errors.restoreFailed': '恢复 Dream 版本失败，当前记忆没有改动',
  } as Record<string, string>)[key] ?? key,
}));

const summaries = [
  {
    schemaVersion: 1 as const,
    revisionId: 'rev-2',
    runId: 'run-2',
    trigger: 'manual' as const,
    createdAt: '2026-08-08T02:00:00.000Z',
    kind: 'pre_restore' as const,
    restoresRevisionId: 'rev-1',
    bodyChars: 40,
    sectionChars: { facts: 10, today: 0, week: 10, longterm: 20 },
  },
  {
    schemaVersion: 1 as const,
    revisionId: 'rev-1',
    runId: 'run-1',
    trigger: 'automatic' as const,
    createdAt: '2026-08-08T01:00:00.000Z',
    kind: 'dream' as const,
    restoresRevisionId: null,
    bodyChars: 30,
    sectionChars: { facts: 10, today: 0, week: 10, longterm: 10 },
  },
];

/** 后端现读的当前记忆快照：与 revision.before 同构。 */
const currentSnapshot = {
  facts: '- facts current',
  today: 'today stays intact',
  weekDays: [{ date: '2026-08-07', body: 'week current' }],
  longterm: '- longterm current',
};

function revisionBefore(revisionId: string) {
  return {
    facts: `- facts ${revisionId}`,
    today: 'today stays intact',
    weekDays: [{ date: '2026-08-07', body: `week ${revisionId}` }],
    longterm: `- longterm ${revisionId}`,
  };
}

function revisionButton(text: string) {
  return screen.getByText(text).closest('button')!;
}

describe('DreamRevisionBrowser', () => {
  beforeEach(() => {
    vi.mocked(loadDreamRevisions).mockResolvedValue(summaries);
    vi.mocked(loadDreamRevision).mockImplementation(async (_agentId, revisionId) => ({
      revision: {
        ...summaries.find((item) => item.revisionId === revisionId)!,
        before: revisionBefore(revisionId),
      },
      current: structuredClone(currentSnapshot),
    }));
    vi.mocked(restoreDream).mockResolvedValue({ ok: true });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('opens the revision list with kind labels and switches the selected revision', async () => {
    render(<DreamRevisionBrowser agentId="hana" open onClose={vi.fn()} />);

    expect(await screen.findByText('+ - facts rev-2')).toBeInTheDocument();
    const revisionButtons = screen.getAllByRole('button').filter((button) =>
      button.textContent?.includes('settings.memory.dream.revisions.characters'));
    expect(revisionButtons[0]).toHaveTextContent('settings.memory.dream.revisions.preRestore');
    expect(revisionButtons[1]).toHaveTextContent('settings.memory.dream.revisions.automatic');

    fireEvent.click(revisionButtons[1]);
    expect(await screen.findByText('+ - facts rev-1')).toBeInTheDocument();
  });

  it('A: renders the current-vs-revision diff with added/removed marks and unchanged sections', async () => {
    render(<DreamRevisionBrowser agentId="hana" open onClose={vi.fn()} />);

    // added = 恢复后会出现（revision 独有）；removed = 恢复后会移除（current 独有）
    expect(await screen.findByText('+ - facts rev-2')).toBeInTheDocument();
    expect(screen.getByText('- - facts current')).toBeInTheDocument();
    expect(screen.getByText('+ week rev-2')).toBeInTheDocument();
    expect(screen.getByText('- week current')).toBeInTheDocument();
    expect(screen.getByText('+ - longterm rev-2')).toBeInTheDocument();
    // today 两侧一致 → 段落级"无变化"标注
    expect(screen.getByText('settings.memory.dream.revisions.sectionUnchanged')).toBeInTheDocument();
    // diff 图例说明两种标记的含义
    expect(screen.getByText('settings.memory.dream.revisions.diffLegendAdded')).toBeInTheDocument();
    expect(screen.getByText('settings.memory.dream.revisions.diffLegendRemoved')).toBeInTheDocument();
  });

  it('B/C/D: never restores before an explicit second confirmation', async () => {
    render(<DreamRevisionBrowser agentId="hana" open onClose={vi.fn()} />);
    expect(await screen.findByText('+ - facts rev-2')).toBeInTheDocument();

    // B：只打开/选中 revision 不会触发 restore
    expect(restoreDream).not.toHaveBeenCalled();

    // C：第一次点击只进入确认态（并且先现取一次最新 current）
    fireEvent.click(revisionButton('settings.memory.dream.revisions.restoreThis'));
    expect(await screen.findByText('settings.memory.dream.revisions.confirmHint')).toBeInTheDocument();
    expect(restoreDream).not.toHaveBeenCalled();
    await waitFor(() => expect(loadDreamRevision).toHaveBeenCalledTimes(2));

    // D：第二次明确确认才调用 restore
    fireEvent.click(revisionButton('settings.memory.dream.revisions.confirmRestore'));
    await waitFor(() => expect(restoreDream).toHaveBeenCalledTimes(1));
    expect(restoreDream).toHaveBeenCalledWith('hana', 'rev-2');
  });

  it('E: shows a no-difference state and disables restore when current equals the revision', async () => {
    vi.mocked(loadDreamRevision).mockImplementation(async (_agentId, revisionId) => ({
      revision: {
        ...summaries.find((item) => item.revisionId === revisionId)!,
        before: structuredClone(currentSnapshot),
      },
      current: structuredClone(currentSnapshot),
    }));
    render(<DreamRevisionBrowser agentId="hana" open onClose={vi.fn()} />);

    expect(await screen.findByText('settings.memory.dream.revisions.noDifference')).toBeInTheDocument();
    expect(revisionButton('settings.memory.dream.revisions.restoreThis')).toBeDisabled();
    // 四段全部"无变化"
    expect(screen.getAllByText('settings.memory.dream.revisions.sectionUnchanged')).toHaveLength(4);
  });

  it('F: refreshes the revision list and the current comparison after a successful restore', async () => {
    render(<DreamRevisionBrowser agentId="hana" open onClose={vi.fn()} />);
    expect(await screen.findByText('+ - facts rev-2')).toBeInTheDocument();

    fireEvent.click(revisionButton('settings.memory.dream.revisions.restoreThis'));
    fireEvent.click(await screen.findByText('settings.memory.dream.revisions.confirmRestore'));

    expect(await screen.findByText('settings.memory.dream.revisions.restored')).toBeInTheDocument();
    await waitFor(() => {
      // 列表被重新拉取（初始一次 + 恢复后一次）
      expect(loadDreamRevisions).toHaveBeenCalledTimes(2);
      // 恢复后重新读取 revision detail（刷新 current 对比）
      expect(vi.mocked(loadDreamRevision).mock.calls.length).toBeGreaterThanOrEqual(3);
    });
    // 最后一次 detail 读取必须发生在 restore 之后
    const restoreOrder = vi.mocked(restoreDream).mock.invocationCallOrder[0];
    const revisionCalls = vi.mocked(loadDreamRevision).mock.invocationCallOrder;
    expect(revisionCalls[revisionCalls.length - 1]).toBeGreaterThan(restoreOrder);
  });

  it('shows a localized restore error instead of the backend English detail', async () => {
    const codedError = Object.assign(new Error('Dream revision was not found'), {
      code: 'dream_revision_not_found',
    });
    vi.mocked(restoreDream).mockRejectedValueOnce(codedError);
    render(<DreamRevisionBrowser agentId="hana" open onClose={vi.fn()} />);

    expect(await screen.findByText('+ - facts rev-2')).toBeInTheDocument();
    fireEvent.click(revisionButton('settings.memory.dream.revisions.restoreThis'));
    fireEvent.click(await screen.findByText('settings.memory.dream.revisions.confirmRestore'));

    expect(await screen.findByText('找不到这个 Dream 版本，它可能已经被清理')).toBeInTheDocument();
    expect(screen.queryByText('Dream revision was not found')).not.toBeInTheDocument();
  });
});
