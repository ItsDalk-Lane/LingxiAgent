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

    // 默认是整洁视图，没有任何勾选框；点行标题进入选择模式并勾中该行
    expect(screen.queryAllByRole('checkbox')).toHaveLength(0);
    fireEvent.click(screen.getByText('Alpha'));
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

    // 进入选择模式后全选框才出现（第 0 个全选、第 1 个未归属组勾选、第 2、3 个行勾选）
    fireEvent.click(screen.getByText('Alpha'));
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
    expect(screen.getByRole('button', { name: /session\.archived\.restoreSelected/ })).toBeDisabled();
  });

  it('restores checked sessions in batch after confirm without switching sessions', async () => {
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
    restoreMock.mockResolvedValue({ status: 'ok', restoredPath: '/x/a.jsonl', sessionId: 'sess_archived_a' });
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('Alpha'));

    fireEvent.click(screen.getByText('Alpha'));
    fireEvent.click(screen.getByRole('button', { name: /session\.archived\.restoreSelected/ }));

    await waitFor(() => expect(restoreMock).toHaveBeenCalledTimes(1));
    expect(restoreMock).toHaveBeenCalledWith(
      expect.objectContaining({ path: '/x/a.jsonl' }),
      { switchTo: false },
    );
    await waitFor(() => expect(toastMock).toHaveBeenCalledWith(
      'session.archived.restoreSelectedDone[{"count":1}]',
    ));
  });

  it('shows partial toast when some batch restores conflict', async () => {
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
    restoreMock
      .mockResolvedValueOnce({ status: 'ok', restoredPath: '/x/a.jsonl', sessionId: 'sess_archived_a' })
      .mockResolvedValueOnce({ status: 'conflict' });
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => screen.getByText('Alpha'));

    fireEvent.click(screen.getByText('Alpha'));
    fireEvent.click(screen.getAllByRole('checkbox')[0]);
    fireEvent.click(screen.getByRole('button', { name: /session\.archived\.restoreSelected/ }));

    await waitFor(() => expect(restoreMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(toastMock).toHaveBeenCalledWith(
      'session.archived.restoreSelectedPartial[{"restored":1,"total":2}]',
    ));
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

  it('deletes an entire group after selecting it via the group header click', async () => {
    listMock.mockResolvedValue(groupedItems());
    deleteMock.mockResolvedValue(true);
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText('B-1')).toBeInTheDocument());

    // 点分组标题 → 进入选择模式并勾中整组 → 删除所选
    fireEvent.click(screen.getByText('工作台B'));
    fireEvent.click(screen.getByRole('button', { name: /session\.archived\.deleteSelected/ }));

    await waitFor(() => expect(deleteMock).toHaveBeenCalledTimes(1));
    expect(deleteMock).toHaveBeenCalledWith(expect.objectContaining({ path: '/arch/mount-b.jsonl' }));
    expect(toastMock).toHaveBeenCalledWith('session.archived.deleteSelectedDone[{"count":1}]');
  });

  it('restores an entire group after selecting it via the group header click', async () => {
    listMock.mockResolvedValue(groupedItems());
    restoreMock.mockResolvedValue({ status: 'ok', restoredPath: '/arch/mount-b.jsonl', sessionId: 's1' });
    window.confirm = vi.fn(() => true);
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText('B-1')).toBeInTheDocument());

    fireEvent.click(screen.getByText('工作台B'));
    fireEvent.click(screen.getByRole('button', { name: /session\.archived\.restoreSelected/ }));

    await waitFor(() => expect(restoreMock).toHaveBeenCalledTimes(1));
    expect(restoreMock).toHaveBeenCalledWith(
      expect.objectContaining({ path: '/arch/mount-b.jsonl' }),
      { switchTo: false },
    );
    expect(toastMock).toHaveBeenCalledWith('session.archived.restoreSelectedDone[{"count":1}]');
  });

  it('reveals checkboxes only in selection mode and preselects the clicked group', async () => {
    listMock.mockResolvedValue(groupedItems());
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText('B-1')).toBeInTheDocument());

    // 打开时无勾选框；点组头后勾选框出现，且整组（含全选框）可见
    expect(screen.queryAllByRole('checkbox')).toHaveLength(0);
    fireEvent.click(screen.getByText('工作台B'));
    const boxes = screen.getAllByRole('checkbox');
    // 顺序：全选0、组:工作台B1、行B-1 2、组:旧工作台3、行Gone-1 4、组:OH 5、行D-1 6、组:未归属7、行N-1 8
    expect(boxes).toHaveLength(9);
    expect((boxes[1] as HTMLInputElement).checked).toBe(true);
    expect((boxes[2] as HTMLInputElement).checked).toBe(true);
    expect((boxes[8] as HTMLInputElement).checked).toBe(false);
  });

  it('toggles a whole group via the group checkbox after entering selection mode', async () => {
    listMock.mockResolvedValue(groupedItems());
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText('B-1')).toBeInTheDocument());

    // 先点行进入选择模式，再用组勾选框勾中未归属组
    fireEvent.click(screen.getByText('B-1'));
    const groupBlocks = screen.getAllByRole('checkbox');
    // 顺序：全选0、组:工作台B1、行B-1 2、组:旧工作台3、行Gone-1 4、组:OH 5、行D-1 6、组:未归属7、行N-1 8
    fireEvent.click(groupBlocks[7]);
    expect((screen.getAllByRole('checkbox')[8] as HTMLInputElement).checked).toBe(true);
    const deleteSelected = screen.getByRole('button', { name: /session\.archived\.deleteSelected/ });
    expect(deleteSelected.textContent).toContain('2');
  });

  it('exits selection mode via the cancel button and clears the selection', async () => {
    listMock.mockResolvedValue(groupedItems());
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText('B-1')).toBeInTheDocument());

    fireEvent.click(screen.getByText('工作台B'));
    expect(screen.getAllByRole('checkbox').length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole('button', { name: 'session.archived.exitSelection' }));
    expect(screen.queryAllByRole('checkbox')).toHaveLength(0);
    expect(screen.getByRole('button', { name: /session\.archived\.deleteSelected/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /session\.archived\.restoreSelected/ })).toBeDisabled();
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

  it('collapses and expands a whole group via the chevron without entering selection mode', async () => {
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
    const chevron = container.querySelector('[data-group-chevron="mount:local_fs_b"]') as HTMLElement;
    expect(header).toBeTruthy();
    expect(header.getAttribute('aria-expanded')).toBe('true');

    // 折叠：组内记录整组收起，组头仍在，且不进入选择模式
    fireEvent.click(chevron);
    expect(screen.queryByText('B-Row')).toBeNull();
    expect(screen.getByText('工作台B')).toBeInTheDocument();
    expect(header.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryAllByRole('checkbox')).toHaveLength(0);

    // 展开：记录回来
    fireEvent.click(chevron);
    expect(await screen.findByText('B-Row')).toBeInTheDocument();
    expect(header.getAttribute('aria-expanded')).toBe('true');
  });

  it('keeps row action buttons working without entering selection mode or collapsing', async () => {
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

    fireEvent.click(screen.getByText('session.archived.deleteForever'));

    await waitFor(() => expect(deleteMock).toHaveBeenCalledTimes(1));
    // 行内按钮不冒泡：不进入选择模式、不触发行勾选、也不折叠分组
    expect(screen.queryAllByRole('checkbox')).toHaveLength(0);
    const header = container.querySelector('[data-group-header="mount:local_fs_b"]') as HTMLElement;
    expect(header.getAttribute('aria-expanded')).toBe('true');
  });

  it('collapses via any header spot except the group name, which instead reveals checkboxes', async () => {
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

    // 点分组头本体（非目录名称）→ 折叠，且不进入选择模式
    fireEvent.click(header);
    expect(screen.queryByText('B-Row')).toBeNull();
    expect(screen.queryAllByRole('checkbox')).toHaveLength(0);

    // 再点分组头 → 展开
    fireEvent.click(header);
    expect(await screen.findByText('B-Row')).toBeInTheDocument();

    // 点目录名称 → 进入选择模式（勾选框出现），且不折叠
    fireEvent.click(screen.getByText('工作台B'));
    expect(screen.getAllByRole('checkbox').length).toBeGreaterThan(0);
    expect(screen.getByText('B-Row')).toBeInTheDocument();
  });

  it('only the row title reveals checkboxes; other row spots do nothing before selection mode', async () => {
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
    render(<ArchivedSessionsModal open={true} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText('B-Row')).toBeInTheDocument());

    // 点行内非标题区域（元信息「Hana · 今天 · 大小」）→ 不进入选择模式
    fireEvent.click(screen.getByText(/Hana · /));
    expect(screen.queryAllByRole('checkbox')).toHaveLength(0);
    expect(screen.getByRole('button', { name: /session\.archived\.deleteSelected/ })).toBeDisabled();

    // 点标题名称 → 进入选择模式并勾中该行
    fireEvent.click(screen.getByText('B-Row'));
    expect(screen.getAllByRole('checkbox').length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: /session\.archived\.deleteSelected/ })).toBeEnabled();
  });
});
