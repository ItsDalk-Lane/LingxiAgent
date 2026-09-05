/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { WorkspaceSwitcher } from '../../components/right-workspace/WorkspaceSwitcher';

const mocks = vi.hoisted(() => ({
  lingxiFetch: vi.fn(async (_path: string, _opts?: RequestInit) => new Response(JSON.stringify({ ok: true }), { status: 200 })),
  loadModels: vi.fn(),
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (path: string, opts?: RequestInit) => mocks.lingxiFetch(path, opts),
}));

vi.mock('../../utils/ui-helpers', () => ({
  loadModels: () => mocks.loadModels(),
}));

const tMap: Record<string, string> = {
  'desk.title': '工作台',
  'input.selectWorkspace': '选择工作台',
  'input.selectOtherFolder': '选择其他文件夹',
  'input.removeStudioWorkspace': '移除工作台',
  'input.removeRecentWorkspace': '从列表移除',
};

type MockWorkspace = { mountId: string; [key: string]: unknown };

function mockWorkspacesResponse(initialWorkspaces?: MockWorkspace[]) {
  let workspaces: MockWorkspace[] = initialWorkspaces ?? [
    { workspaceId: 'default', mountId: 'default', label: 'Default', isDefault: true, nativeRootPath: '/ws/hana-home' },
    { workspaceId: 'local_fs_b', mountId: 'local_fs_b', label: '工作台B', nativeRootPath: '/ws/b' },
  ];
  mocks.lingxiFetch.mockImplementation(async (path: string, opts?: RequestInit) => {
    if (path === '/api/studio/workspaces') {
      return new Response(JSON.stringify({ workspaces }), { status: 200 });
    }
    if (path === '/api/studio/workspaces/local_fs_b' && opts?.method === 'DELETE') {
      workspaces = workspaces.filter((w) => w.mountId !== 'local_fs_b');
      return new Response(JSON.stringify({ ok: true }), { status: 200 });
    }
    if (path.startsWith('/api/config/workspaces/recent')) {
      return new Response(JSON.stringify({ ok: true, cwd_history: [] }), { status: 200 });
    }
    if (path.startsWith('/api/preferences/workspace-ui-state')) {
      return new Response(JSON.stringify({ state: null }), { status: 200 });
    }
    if (path === '/api/workbench/files?mountId=local_fs_b') {
      return new Response(JSON.stringify({
        mountId: 'local_fs_b',
        mount: { label: '工作台B', nativeRootPath: '/ws/b' },
        files: [],
      }), { status: 200 });
    }
    if (path === '/api/workbench/files?mountId=default') {
      return new Response(JSON.stringify({
        mountId: 'default',
        mount: { label: 'Default', nativeRootPath: '/ws/hana-home' },
        files: [],
      }), { status: 200 });
    }
    if (path.startsWith('/api/workbench/content')) {
      return new Response('', { status: 404 });
    }
    return new Response(JSON.stringify({ ok: true }), { status: 200 });
  });
}

function menuEl(): HTMLElement | null {
  return document.querySelector('[data-right-workspace-switcher-menu]');
}

describe('WorkspaceSwitcher (workspace title dropdown)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.t = ((key: string) => tMap[key] || key) as typeof window.t;
    window.platform = { selectFolder: vi.fn() } as unknown as typeof window.platform;
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it('lists mounts and history folders in one menu, hides the default row and dedupes mount roots', async () => {
    mockWorkspacesResponse();
    useStore.setState({
      serverPort: 62950,
      serverToken: 'test-token',
      deskBasePath: 'studio:default',
      deskWorkspaceMountId: 'default',
      deskWorkspaceLabel: 'hana-home',
      deskWorkspaceNativeRoot: '/ws/hana-home',
      selectedFolder: null,
      selectedWorkspaceMountId: 'default',
      studioWorkspaces: [],
      homeFolder: '/ws/hana-home',
      cwdHistory: ['/ws/b', '/ws/hana-home', '/ws/other'],
      agents: [{ id: 'hana', name: 'Hanako', yuan: 'lingxi', homeFolder: '/ws/hana-home' }],
      currentAgentId: 'hana',
    } as never);

    render(<WorkspaceSwitcher />);
    const trigger = screen.getByRole('button', { name: '选择工作台' });
    expect(trigger).toHaveTextContent('hana-home');
    fireEvent.click(trigger);

    expect(menuEl()).not.toBeNull();
    expect(trigger).toHaveAttribute('aria-expanded', 'true');
    // 打开即刷新挂载列表
    await waitFor(() => expect(useStore.getState().studioWorkspaces).toHaveLength(2));

    const menu = menuEl() as HTMLElement;
    // 与欢迎页选择器同口径：Default 不单列；挂载根不以历史条目重复出现
    expect(within(menu).queryByText('Default')).toBeNull();
    expect(within(menu).getByText('工作台B')).toBeInTheDocument();
    expect(within(menu).getAllByText('hana-home')).toHaveLength(1);
    expect(within(menu).getByText('other')).toBeInTheDocument();
    // 本地 folder picker 可用时保留「选择其他文件夹」入口
    expect(within(menu).getByText('选择其他文件夹')).toBeInTheDocument();
  });

  it('switches to a mount workspace with one click', async () => {
    mockWorkspacesResponse();
    useStore.setState({
      serverPort: 62950,
      serverToken: 'test-token',
      deskBasePath: 'studio:default',
      deskWorkspaceMountId: 'default',
      deskWorkspaceLabel: 'hana-home',
      deskWorkspaceNativeRoot: '/ws/hana-home',
      selectedFolder: null,
      selectedWorkspaceMountId: 'default',
      studioWorkspaces: [],
      homeFolder: '/ws/hana-home',
      cwdHistory: [],
      agents: [],
      currentAgentId: 'hana',
    } as never);

    render(<WorkspaceSwitcher />);
    fireEvent.click(screen.getByRole('button', { name: '选择工作台' }));
    await waitFor(() => expect(useStore.getState().studioWorkspaces).toHaveLength(2));
    fireEvent.click(within(menuEl() as HTMLElement).getByText('工作台B'));

    await waitFor(() => {
      expect(useStore.getState().selectedWorkspaceMountId).toBe('local_fs_b');
      expect(useStore.getState().deskBasePath).toBe('studio:local_fs_b');
    });
    expect(mocks.lingxiFetch.mock.calls.some(([path]) => path === '/api/workbench/files?mountId=local_fs_b')).toBe(true);
    expect(menuEl()).toBeNull();
  });

  it('routes an agent home folder entry through the default mount', async () => {
    mockWorkspacesResponse();
    useStore.setState({
      serverPort: 62950,
      serverToken: 'test-token',
      deskBasePath: '/ws/other',
      deskWorkspaceMountId: null,
      deskWorkspaceLabel: null,
      selectedFolder: '/ws/other',
      selectedWorkspaceMountId: null,
      studioWorkspaces: [],
      homeFolder: null,
      cwdHistory: ['/ws/hana-home'],
      agents: [{ id: 'hana', name: 'Hanako', yuan: 'lingxi', homeFolder: '/ws/hana-home' }],
      currentAgentId: 'hana',
      selectedAgentId: null,
    } as never);

    render(<WorkspaceSwitcher />);
    fireEvent.click(screen.getByRole('button', { name: '选择工作台' }));
    fireEvent.click(screen.getByText('hana-home'));

    // Agent 主目录 = 默认工作台解析根：统一 mount 形态，与经欢迎页切换的会话同一本账
    await waitFor(() => expect(useStore.getState().selectedWorkspaceMountId).toBe('default'));
    expect(useStore.getState().selectedFolder).toBeNull();
    expect(mocks.lingxiFetch.mock.calls.some(([path]) => path === '/api/workbench/files?mountId=default')).toBe(true);
  });

  it('does not reset the session when the active workspace is clicked again', async () => {
    mockWorkspacesResponse();
    useStore.setState({
      serverPort: 62950,
      serverToken: 'test-token',
      deskBasePath: 'studio:local_fs_b',
      deskWorkspaceMountId: 'local_fs_b',
      deskWorkspaceLabel: '工作台B',
      deskWorkspaceNativeRoot: '/ws/b',
      selectedFolder: null,
      selectedWorkspaceMountId: 'local_fs_b',
      studioWorkspaces: [],
      homeFolder: null,
      cwdHistory: [],
      agents: [],
      currentAgentId: 'hana',
    } as never);

    render(<WorkspaceSwitcher />);
    fireEvent.click(screen.getByRole('button', { name: '选择工作台' }));
    await waitFor(() => expect(useStore.getState().studioWorkspaces).toHaveLength(2));

    const workbenchCallsBefore = mocks.lingxiFetch.mock.calls.filter(([path]) => String(path).startsWith('/api/workbench/files')).length;
    fireEvent.click(within(menuEl() as HTMLElement).getByText('工作台B'));

    expect(menuEl()).toBeNull();
    expect(useStore.getState().selectedWorkspaceMountId).toBe('local_fs_b');
    const workbenchCallsAfter = mocks.lingxiFetch.mock.calls.filter(([path]) => String(path).startsWith('/api/workbench/files')).length;
    expect(workbenchCallsAfter).toBe(workbenchCallsBefore);
  });

  it('removes a user-added mount from the menu with the same semantics as the welcome picker', async () => {
    mockWorkspacesResponse();
    useStore.setState({
      serverPort: 62950,
      serverToken: 'test-token',
      deskBasePath: 'studio:default',
      deskWorkspaceMountId: 'default',
      deskWorkspaceLabel: 'hana-home',
      deskWorkspaceNativeRoot: '/ws/hana-home',
      selectedFolder: null,
      selectedWorkspaceMountId: 'default',
      studioWorkspaces: [],
      homeFolder: '/ws/hana-home',
      cwdHistory: [],
      sessions: [],
      agents: [],
      currentAgentId: 'hana',
    } as never);

    render(<WorkspaceSwitcher />);
    fireEvent.click(screen.getByRole('button', { name: '选择工作台' }));
    await waitFor(() => expect(useStore.getState().studioWorkspaces).toHaveLength(2));

    const row = within(menuEl() as HTMLElement).getByText('工作台B').closest('div') as HTMLElement;
    fireEvent.click(within(row).getByRole('button', { name: '移除工作台' }));

    // 无名下对话：直接移除，不归档
    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith(
        '/api/studio/workspaces/local_fs_b',
        expect.objectContaining({ method: 'DELETE' }),
      );
    });
    await waitFor(() => expect(useStore.getState().studioWorkspaces).toHaveLength(1));
    expect(menuEl()).toBeNull();
  });

  it('removes a history entry from the menu without closing it', async () => {
    mockWorkspacesResponse();
    useStore.setState({
      serverPort: 62950,
      serverToken: 'test-token',
      deskBasePath: 'studio:default',
      deskWorkspaceMountId: 'default',
      deskWorkspaceLabel: 'hana-home',
      deskWorkspaceNativeRoot: '/ws/hana-home',
      selectedFolder: null,
      selectedWorkspaceMountId: 'default',
      studioWorkspaces: [],
      homeFolder: '/ws/hana-home',
      cwdHistory: ['/ws/other'],
      sessions: [],
      agents: [],
      currentAgentId: 'hana',
    } as never);

    render(<WorkspaceSwitcher />);
    fireEvent.click(screen.getByRole('button', { name: '选择工作台' }));
    await waitFor(() => expect(useStore.getState().studioWorkspaces).toHaveLength(2));

    const row = within(menuEl() as HTMLElement).getByText('other').closest('div') as HTMLElement;
    fireEvent.click(within(row).getByRole('button', { name: '从列表移除' }));

    expect(mocks.lingxiFetch).toHaveBeenCalledWith(
      '/api/config/workspaces/recent?agentId=hana',
      expect.objectContaining({
        method: 'DELETE',
        body: JSON.stringify({ path: '/ws/other' }),
      }),
    );
    expect(useStore.getState().cwdHistory).toEqual([]);
    // 轻操作：菜单保持展开，行即时消失
    expect(menuEl()).not.toBeNull();
    expect(within(menuEl() as HTMLElement).queryByText('other')).toBeNull();
  });
});
