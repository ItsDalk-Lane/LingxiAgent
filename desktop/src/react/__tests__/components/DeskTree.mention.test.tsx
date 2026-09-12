/**
 * @vitest-environment jsdom
 *
 * 工作台文件树行尾「@ 添加到对话」：点击把该条目按 native 绝对路径加进聊天输入框，
 * 语义与从文件树拖进聊天框一致。没有 native 路径的工作台不提供该入口。
 */

import React from 'react';
import { cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { DeskTree } from '../../components/desk/DeskTree';

const mocks = vi.hoisted(() => ({
  loadDeskTreeFiles: vi.fn(async () => {}),
}));

vi.mock('../../stores/desk-actions', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../stores/desk-actions')>();
  return {
    ...actual,
    loadDeskTreeFiles: mocks.loadDeskTreeFiles,
  };
});

function renderTree() {
  return render(
    <DeskTree
      sortMode="name-asc"
      onShowMenu={vi.fn()}
      inlineEdit={null}
      onInlineEditChange={vi.fn()}
      onStartCreate={vi.fn(async () => {})}
    />,
  );
}

function mentionButtonFor(container: HTMLElement, deskPath: string): HTMLElement | null {
  return container
    .querySelector(`[data-desk-path="${deskPath}"]`)
    ?.querySelector('[data-desk-mention]') as HTMLElement | null;
}

describe('DeskTree @ add-to-chat', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.t = ((key: string) => key) as typeof window.t;
    useStore.setState({
      activeServerConnection: null,
      activeServerConnectionId: null,
      serverConnections: {},
      serverPort: 62950,
      serverToken: 'local-token',
      currentTab: 'chat',
      attachedFiles: [],
      attachedFilesBySession: {},
      deskBasePath: '/Users/me/project',
      deskWorkspaceMountId: null,
      deskWorkspaceNativeRoot: null,
      deskFiles: [
        { name: 'notes', isDir: true },
        { name: 'report.md', isDir: false },
      ],
      deskTreeFilesByPath: {
        '': [
          { name: 'notes', isDir: true },
          { name: 'report.md', isDir: false },
        ],
        notes: [{ name: 'chapter.md', isDir: false }],
      },
      deskExpandedPaths: ['notes'],
      deskSelectedPath: '',
    } as never);
  });

  afterEach(() => {
    cleanup();
  });

  it('adds a hovered file row to the composer with its absolute path', () => {
    const { container } = renderTree();
    const button = mentionButtonFor(container, 'report.md');

    expect(button).not.toBeNull();
    fireEvent.click(button as Element);

    expect(useStore.getState().attachedFiles).toEqual([
      { path: '/Users/me/project/report.md', name: 'report.md', isDirectory: false },
    ]);
  });

  it('adds a folder row as a directory attachment', () => {
    const { container } = renderTree();
    fireEvent.click(mentionButtonFor(container, 'notes') as Element);

    expect(useStore.getState().attachedFiles).toEqual([
      { path: '/Users/me/project/notes', name: 'notes', isDirectory: true },
    ]);
  });

  it('resolves nested rows against the native root of a local_fs mount', () => {
    useStore.setState({
      deskBasePath: 'studio:mount_docs',
      deskWorkspaceMountId: 'mount_docs',
      deskWorkspaceLabel: 'Docs',
      deskWorkspaceNativeRoot: '/Users/me/docs',
    } as never);

    const { container } = renderTree();
    fireEvent.click(mentionButtonFor(container, 'notes/chapter.md') as Element);

    expect(useStore.getState().attachedFiles).toEqual([
      { path: '/Users/me/docs/notes/chapter.md', name: 'chapter.md', isDirectory: false },
    ]);
  });

  it('keeps the row click behavior untouched when the @ button is clicked', () => {
    const { container } = renderTree();
    const row = container.querySelector('[data-desk-path="notes"]') as Element;

    fireEvent.click(mentionButtonFor(container, 'notes') as Element);

    // 行自身的 onClick（选中 + 折叠展开）不因 @ 按钮被触发。
    expect(useStore.getState().deskSelectedPath).toBe('');
    expect(useStore.getState().deskExpandedPaths).toEqual(['notes']);
    expect(row.getAttribute('data-selected')).toBe('false');
  });

  it('does not attach while another tab is active', () => {
    useStore.setState({ currentTab: 'knowledge' } as never);

    const { container } = renderTree();
    fireEvent.click(mentionButtonFor(container, 'report.md') as Element);

    expect(useStore.getState().attachedFiles).toEqual([]);
  });

  it('offers no @ entry for mounts without a native root', () => {
    useStore.setState({
      deskBasePath: 'studio:mount_remote',
      deskWorkspaceMountId: 'mount_remote',
      deskWorkspaceLabel: 'Remote',
      deskWorkspaceNativeRoot: null,
    } as never);

    const { container } = renderTree();

    expect(container.querySelector('[data-desk-mention]')).toBeNull();
  });

  it('offers no @ entry to a remote client viewing a local workspace', () => {
    useStore.setState({
      activeServerConnection: {
        connectionId: 'browser:server_lan',
        kind: 'lan',
        serverId: 'server_lan',
        userId: 'user_lan',
        studioId: 'studio_lan',
        label: 'LAN Hana',
        baseUrl: 'http://hana.local:14500',
        wsUrl: 'ws://hana.local:14500',
        token: null,
        authState: 'paired',
        trustState: 'lan',
        credentialKind: 'device_credential',
        platformAccountId: null,
        officialServiceKind: null,
        capabilities: ['resources', 'files'],
      },
      deskBasePath: '/Users/server/project',
      deskWorkspaceMountId: null,
      deskWorkspaceNativeRoot: null,
    } as never);

    const { container } = renderTree();

    expect(container.querySelector('[data-desk-mention]')).toBeNull();
  });
});
