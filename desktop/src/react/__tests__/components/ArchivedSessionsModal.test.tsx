/**
 * @vitest-environment jsdom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

const listMock = vi.fn();
const restoreMock = vi.fn();
const deleteMock = vi.fn();
const cleanupMock = vi.fn();
const toastMock = vi.fn();

vi.mock('../../stores/session-actions', () => ({
  listArchivedSessions: (...args: unknown[]) => listMock(...args),
  restoreSession: (...args: unknown[]) => restoreMock(...args),
  deleteArchivedSession: (...args: unknown[]) => deleteMock(...args),
  cleanupArchivedSessions: (...args: unknown[]) => cleanupMock(...args),
  showSidebarToast: (...args: unknown[]) => toastMock(...args),
}));

vi.mock('../../hooks/use-i18n', () => ({
  useI18n: () => ({
    t: (k: string, v?: Record<string, unknown>) =>
      v ? `${k}[${JSON.stringify(v)}]` : k,
  }),
}));

import { ArchivedSessionsModal } from '../../components/ArchivedSessionsModal';
import { useStore } from '../../stores';

beforeEach(() => {
  listMock.mockReset();
  restoreMock.mockReset();
  deleteMock.mockReset();
  cleanupMock.mockReset();
  toastMock.mockReset();
});

afterEach(() => {
  cleanup();
});

describe('ArchivedSessionsModal', () => {
  it('renders empty state when list is empty', async () => {
    listMock.mockResolvedValue([]);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => {
      expect(screen.getByText('session.archived.empty')).toBeInTheDocument();
    });
  });

  it('renders rows for each archived session', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        sessionId: 'sess_archived_a',
        title: 'Alpha',
        archivedAt: new Date(Date.now() - 2 * 86400_000).toISOString(),
        sizeBytes: 1024 * 1024,
        agentId: 'a',
        agentName: 'Hana',
      },
      {
        path: '/x/b.jsonl',
        title: 'Beta',
        archivedAt: new Date(Date.now() - 10 * 86400_000).toISOString(),
        sizeBytes: 2 * 1024 * 1024,
        agentId: 'b',
        agentName: 'Yuan',
      },
    ]);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => {
      expect(screen.getByText('Alpha')).toBeInTheDocument();
      expect(screen.getByText('Beta')).toBeInTheDocument();
    });
  });

  it('falls back to firstMessage when title is missing', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        title: null,
        firstMessage: 'First user message',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
      {
        path: '/x/b.jsonl',
        title: null,
        firstMessage: null,
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
    ]);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => {
      expect(screen.getByText('First user message')).toBeInTheDocument();
    });
    expect(screen.getByText('session.untitled')).toBeInTheDocument();
    expect(screen.queryByText('session.archived.empty')).not.toBeInTheDocument();
  });

  it('deletes only the checked sessions after confirm', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        sessionId: 'sess_archived_a',
        title: 'Alpha',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
      {
        path: '/x/b.jsonl',
        sessionId: 'sess_archived_b',
        title: 'Beta',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
    ]);
    deleteMock.mockResolvedValue(true);
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('Alpha'));

    const checkboxes = screen.getAllByRole('checkbox');
    // 第 0 个是「全选」，第 1 个是未归属分组的组级勾选（两条无身份记录同组），
    // 第 2、3 个是行勾选框
    fireEvent.click(checkboxes[2]);
    const deleteSelected = screen.getByRole('button', { name: /session\.archived\.deleteSelected/ });
    fireEvent.click(deleteSelected);

    await waitFor(() => expect(deleteMock).toHaveBeenCalledTimes(1));
    expect(deleteMock).toHaveBeenCalledWith(expect.objectContaining({ path: '/x/a.jsonl' }));
    await waitFor(() => expect(toastMock).toHaveBeenCalledWith(
      'session.archived.deleteSelectedDone[{"count":1}]',
    ));
  });

  it('selects all rows via the select-all checkbox', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        sessionId: 'sess_archived_a',
        title: 'Alpha',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
      {
        path: '/x/b.jsonl',
        sessionId: 'sess_archived_b',
        title: 'Beta',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
    ]);
    deleteMock.mockResolvedValue(true);
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('Alpha'));

    fireEvent.click(screen.getAllByRole('checkbox')[0]);
    fireEvent.click(screen.getByRole('button', { name: /session\.archived\.deleteSelected/ }));

    await waitFor(() => expect(deleteMock).toHaveBeenCalledTimes(2));
    expect(deleteMock).toHaveBeenCalledWith(expect.objectContaining({ path: '/x/a.jsonl' }));
    expect(deleteMock).toHaveBeenCalledWith(expect.objectContaining({ path: '/x/b.jsonl' }));
  });

  it('keeps delete-selected disabled when nothing is checked', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        title: 'Alpha',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
    ]);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('Alpha'));
    expect(screen.getByRole('button', { name: /session\.archived\.deleteSelected/ })).toBeDisabled();
  });

  it('returns null when closed (no render side-effect)', () => {
    listMock.mockResolvedValue([]);
    const { container } = render(<ArchivedSessionsModal open={false} onClose={() => {}} />);
    expect(container.textContent).toBe('');
    expect(listMock).not.toHaveBeenCalled();
  });

  it('calls restoreSession with user confirmation', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        sessionId: 'sess_archived_a',
        title: 'Alpha',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
    ]);
    restoreMock.mockResolvedValue({ status: 'ok', restoredPath: '/x/a.jsonl', sessionId: 'sess_archived_a' });
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('Alpha'));
    fireEvent.click(screen.getByText('session.archived.restore'));
    await waitFor(() => expect(restoreMock).toHaveBeenCalledWith(expect.objectContaining({
      path: '/x/a.jsonl',
      sessionId: 'sess_archived_a',
    })));
  });

  it('skips restore when user cancels confirm', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        title: 'Alpha',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
    ]);
    window.confirm = vi.fn(() => false);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('Alpha'));
    fireEvent.click(screen.getByText('session.archived.restore'));
    expect(restoreMock).not.toHaveBeenCalled();
  });

  it('shows conflict toast when restore returns conflict', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        title: 'Alpha',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
    ]);
    restoreMock.mockResolvedValue({ status: 'conflict' });
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('Alpha'));
    fireEvent.click(screen.getByText('session.archived.restore'));
    await waitFor(() =>
      expect(toastMock).toHaveBeenCalledWith('session.archived.restoreConflict'),
    );
  });

  it('calls cleanupArchivedSessions(30) when 30-day button clicked', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        sessionId: 'sess_archived_delete',
        title: 'A',
        archivedAt: new Date(Date.now() - 40 * 86400_000).toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
    ]);
    cleanupMock.mockResolvedValue({ deleted: 1 });
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('A'));
    fireEvent.click(screen.getByText('session.archived.cleanup30'));
    await waitFor(() => expect(cleanupMock).toHaveBeenCalledWith(30));
  });

  it('skips cleanup confirm when nothing matches', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        sessionId: 'sess_archived_delete',
        title: 'A',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
    ]);
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('A'));
    fireEvent.click(screen.getByText('session.archived.cleanup90'));
    expect(cleanupMock).not.toHaveBeenCalled();
    expect(toastMock).toHaveBeenCalledWith('session.archived.cleanupNoMatch');
  });

  it('calls deleteArchivedSession with confirmation', async () => {
    listMock.mockResolvedValue([
      {
        path: '/x/a.jsonl',
        sessionId: 'sess_archived_delete',
        title: 'A',
        archivedAt: new Date().toISOString(),
        sizeBytes: 100,
        agentId: 'a',
        agentName: 'Hana',
      },
    ]);
    deleteMock.mockResolvedValue(true);
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('A'));
    fireEvent.click(screen.getByText('session.archived.deleteForever'));
    await waitFor(() => expect(deleteMock).toHaveBeenCalledWith(expect.objectContaining({
      path: '/x/a.jsonl',
      sessionId: 'sess_archived_delete',
    })));
  });
});

describe('ArchivedSessionsModal workspace grouping', () => {
  beforeEach(() => {
    useStore.setState({
      studioWorkspaces: [
        { mountId: 'default', label: 'Default', isDefault: true, nativeRootPath: '/Users/test/Desktop/OH-WorkSpace' },
        { mountId: 'local_fs_b', label: '工作台B', nativeRootPath: '/Users/test/Desktop/B' },
      ],
      defaultWorkspaceRootPath: '/Users/test/Desktop/OH-WorkSpace',
    } as never);
  });

  function groupedItems() {
    return [
      {
        path: '/arch/mount-b.jsonl',
        sessionId: 's1',
        title: 'B-1',
        archivedAt: new Date().toISOString(),
        sizeBytes: 10,
        agentId: 'a',
        agentName: 'Hana',
        workspaceMountId: 'local_fs_b',
        workspaceLabel: '工作台B',
        cwd: '/Users/test/Desktop/B',
      },
      {
        path: '/arch/mount-gone.jsonl',
        sessionId: 's2',
        title: 'Gone-1',
        archivedAt: new Date().toISOString(),
        sizeBytes: 10,
        agentId: 'a',
        agentName: 'Hana',
        workspaceMountId: 'local_fs_gone',
        workspaceLabel: '旧工作台',
        cwd: '/Users/test/Desktop/Gone',
      },
      {
        path: '/arch/default.jsonl',
        sessionId: 's3',
        title: 'D-1',
        archivedAt: new Date().toISOString(),
        sizeBytes: 10,
        agentId: 'a',
        agentName: 'Hana',
        workspaceMountId: 'default',
        workspaceLabel: 'Default',
        cwd: '/Users/test/Desktop/OH-WorkSpace',
      },
      {
        path: '/arch/noidentity.jsonl',
        sessionId: 's4',
        title: 'N-1',
        archivedAt: new Date().toISOString(),
        sizeBytes: 10,
        agentId: 'a',
        agentName: 'Hana',
        workspaceMountId: null,
        cwd: null,
      },
    ];
  }

  it('groups by workspace identity, derives the default display name, and marks removed workspaces', async () => {
    listMock.mockResolvedValue(groupedItems());
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);

    await waitFor(() => expect(screen.getByText('B-1')).toBeInTheDocument());

    // mount 分组标题取 label；default 分组显示名=配置目录名（与主界面规则一致）；
    // 已移除 mount 的分组带「该工作目录已移除」徽标；无身份 → 未归属
    expect(screen.getByText('工作台B')).toBeInTheDocument();
    expect(screen.getByText('OH-WorkSpace')).toBeInTheDocument();
    expect(screen.getByText('旧工作台')).toBeInTheDocument();
    expect(screen.getAllByText('session.archived.group.workspaceRemoved')).toHaveLength(1);
    expect(screen.getByText('session.archived.group.ungrouped')).toBeInTheDocument();
  });

  it('deletes an entire group through the group-level button', async () => {
    listMock.mockResolvedValue(groupedItems());
    deleteMock.mockResolvedValue(true);
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText('B-1')).toBeInTheDocument());

    fireEvent.click(screen.getAllByText('session.archived.deleteGroup')[0]);

    await waitFor(() => expect(deleteMock).toHaveBeenCalledTimes(1));
    expect(deleteMock).toHaveBeenCalledWith(expect.objectContaining({ path: '/arch/mount-b.jsonl' }));
    expect(toastMock).toHaveBeenCalledWith('session.archived.deleteGroupDone[{"count":1}]');
  });

  it('toggles a whole group via the group checkbox', async () => {
    listMock.mockResolvedValue(groupedItems());
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText('B-1')).toBeInTheDocument());

    // 未归属组只有一条（N-1）：组级勾选应选中它
    const groupBlocks = screen.getAllByRole('checkbox');
    // [全选, 组:工作台B, 组:旧工作台, 组:OH-WorkSpace, 组:未归属, 行B-1, 行Gone-1, 行D-1, 行N-1]
    fireEvent.click(groupBlocks[4]);
    const deleteSelected = screen.getByRole('button', { name: /session\.archived\.deleteSelected/ });
    expect(deleteSelected.textContent).toContain('1');
  });
});

describe('ArchivedSessionsModal group collapse', () => {
  beforeEach(() => {
    useStore.setState({
      studioWorkspaces: [
        { mountId: 'default', label: 'Default', isDefault: true, nativeRootPath: '/Users/test/Desktop/OH-WorkSpace' },
        { mountId: 'local_fs_b', label: '工作台B', nativeRootPath: '/Users/test/Desktop/B' },
      ],
      defaultWorkspaceRootPath: '/Users/test/Desktop/OH-WorkSpace',
    } as never);
  });

  it('collapses and expands a whole group via the group header click', async () => {
    listMock.mockResolvedValue([
      {
        path: '/arch/b1.jsonl',
        sessionId: 's1',
        title: 'B-Row',
        archivedAt: new Date().toISOString(),
        sizeBytes: 10,
        agentId: 'a',
        agentName: 'Hana',
        workspaceMountId: 'local_fs_b',
        workspaceLabel: '工作台B',
        cwd: '/Users/test/Desktop/B',
      },
    ]);
    const { ArchivedSessionsModal } = await import('../../components/ArchivedSessionsModal');
    const { container } = render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText('B-Row')).toBeInTheDocument());

    const header = container.querySelector('[data-group-header="mount:local_fs_b"]') as HTMLElement;
    expect(header).toBeTruthy();
    expect(header.getAttribute('aria-expanded')).toBe('true');

    // 折叠：组内记录整组收起，组头仍在
    fireEvent.click(header);
    expect(screen.queryByText('B-Row')).toBeNull();
    expect(screen.getByText('工作台B')).toBeInTheDocument();
    expect(header.getAttribute('aria-expanded')).toBe('false');

    // 展开：记录回来
    fireEvent.click(header);
    expect(await screen.findByText('B-Row')).toBeInTheDocument();
    expect(header.getAttribute('aria-expanded')).toBe('true');
  });

  it('keeps the group delete button working without toggling collapse', async () => {
    listMock.mockResolvedValue([
      {
        path: '/arch/b1.jsonl',
        sessionId: 's1',
        title: 'B-Row',
        archivedAt: new Date().toISOString(),
        sizeBytes: 10,
        agentId: 'a',
        agentName: 'Hana',
        workspaceMountId: 'local_fs_b',
        workspaceLabel: '工作台B',
        cwd: '/Users/test/Desktop/B',
      },
    ]);
    deleteMock.mockResolvedValue(true);
    window.confirm = vi.fn(() => true);
    const { ArchivedSessionsModal } = await import('../../components/ArchivedSessionsModal');
    const { container } = render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText('B-Row')).toBeInTheDocument());

    fireEvent.click(screen.getByText('session.archived.deleteGroup'));

    await waitFor(() => expect(deleteMock).toHaveBeenCalledTimes(1));
    // 删除按钮不触发折叠
    const header = container.querySelector('[data-group-header="mount:local_fs_b"]') as HTMLElement;
    expect(header.getAttribute('aria-expanded')).toBe('true');
  });
});
