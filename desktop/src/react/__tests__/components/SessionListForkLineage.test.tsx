/**
 * 谱系分组（子对话归位）端到端：
 *  - 子对话渲染为主对话下方缩进行（data 层 childOfSessionId → 行 class）；
 *  - 归档带子对话的主对话 → 弹三选一（一起归档 / 仅主对话 / 取消），分别带 childMode 提交；
 *  - 无子对话的归档直接提交，不弹框。
 * 说明：SessionListContextMenu.test.tsx 存在与工作区未完成分组折叠改动相关的
 * 既有失败（非本特性引入），本文件用与现状语义一致的种子独立覆盖本特性。
 */
// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SessionList } from '../../components/SessionList';
import { useStore } from '../../stores';

const { archiveMock, switchSessionMock } = vi.hoisted(() => ({
  archiveMock: vi.fn(async (_path?: string, _childMode?: string) => undefined),
  switchSessionMock: vi.fn(async () => undefined),
}));

vi.mock('../../stores/session-actions', () => ({
  switchSession: switchSessionMock,
  archiveSession: (path: string, childMode?: string) => archiveMock(path, childMode),
  renameSession: vi.fn(async () => undefined),
  pinSession: vi.fn(async () => undefined),
  reorderPinnedSessions: vi.fn(async () => undefined),
  disposeWorkspaceSessions: vi.fn(async () => ({ disposed: 0 })),
  createNewSession: vi.fn(async () => undefined),
  loadSessions: vi.fn(async () => undefined),
}));

function seed(sessions: unknown[]) {
  useStore.setState({
    sessions,
    currentSessionPath: null,
    pendingSessionSwitchPath: null,
    pendingNewSession: false,
    agents: [],
    streamingSessions: [],
    unreadOutputSessionPaths: [],
    failedSessions: [],
    browserBySession: {},
    loopStatusBySession: {},
    sidebarUiPrefsLoaded: true,
    selectedFolder: '/tmp/project',
    selectedWorkspaceMountId: null,
    deskWorkspaceMountId: null,
    deskBasePath: '/tmp/project',
    deskWorkspaceLabel: null,
    studioWorkspaces: [],
  } as never);
}

function session(overrides: Record<string, unknown>) {
  return {
    title: null,
    firstMessage: 'hello',
    modified: '2026-04-29T08:00:00.000Z',
    messageCount: 1,
    agentId: 'hana',
    agentName: 'Hana',
    cwd: '/tmp/project',
    pinnedAt: null,
    ...overrides,
  };
}

const mainSession = session({
  path: '/tmp/agents/hana/sessions/main.jsonl',
  sessionId: 'sess_main',
  firstMessage: '主对话',
});
const forkSession = session({
  path: '/tmp/agents/hana/sessions/fork.jsonl',
  sessionId: 'sess_fork',
  firstMessage: '编辑重发支线',
  forkedFrom: { sessionId: 'sess_main' },
});

describe('谱系分组：列表渲染与归档选择', () => {
  beforeEach(() => {
    globalThis.t = ((key: string) => key) as typeof globalThis.t;
    archiveMock.mockClear();
  });
  afterEach(() => cleanup());

  it('子对话渲染为主对话下方的缩进行', () => {
    seed([mainSession, forkSession]);
    render(<SessionList />);
    const mainRow = screen.getByText('主对话').closest('button');
    const forkRow = screen.getByText('编辑重发支线').closest('button');
    expect(mainRow).not.toBeNull();
    expect(forkRow).not.toBeNull();
    // 主对话在前、子对话紧随其后（DOM 顺序）
    expect(mainRow!.compareDocumentPosition(forkRow as Node) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // 子行带缩进 class，主行不带
    expect(forkRow!.className).toContain('sessionItemChild');
    expect(mainRow!.className).not.toContain('sessionItemChild');
  });

  it('归档带子对话的主对话：弹三选一，选择后带策略提交', async () => {
    seed([mainSession, forkSession]);
    render(<SessionList />);
    const mainRow = screen.getByText('主对话').closest('button');
    fireEvent.contextMenu(mainRow!, { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByText('session.archive'));
    // 三选一对话框出现
    const dialog = await screen.findByTestId('session-archive-choice-dialog');
    expect(dialog).toBeInTheDocument();
    expect(archiveMock).not.toHaveBeenCalled();
    // 仅归档主对话 → detach_children
    fireEvent.click(screen.getByText('session.archiveDetachChildren'));
    await waitFor(() => expect(archiveMock).toHaveBeenCalledWith('/tmp/agents/hana/sessions/main.jsonl', 'detach_children'));
    // 一起归档 → archive_children
    fireEvent.contextMenu(screen.getByText('主对话').closest('button')!, { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByText('session.archive'));
    fireEvent.click(await screen.findByText('session.archiveWithChildren'));
    await waitFor(() => expect(archiveMock).toHaveBeenCalledWith('/tmp/agents/hana/sessions/main.jsonl', 'archive_children'));
  });

  it('无子对话的归档直接提交，不弹选择框', async () => {
    seed([mainSession]);
    render(<SessionList />);
    fireEvent.contextMenu(screen.getByText('主对话').closest('button')!, { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByText('session.archive'));
    await waitFor(() => expect(archiveMock).toHaveBeenCalledWith('/tmp/agents/hana/sessions/main.jsonl', undefined));
    expect(screen.queryByTestId('session-archive-choice-dialog')).not.toBeInTheDocument();
  });
});

describe('谱系折叠：点击助手符号收起/展开子对话', () => {
  beforeEach(() => {
    globalThis.t = ((key: string) => key) as typeof globalThis.t;
    archiveMock.mockClear();
    switchSessionMock.mockClear();
  });
  afterEach(() => cleanup());

  it('点击助手符号：子对话收起且符号显示隐藏数，再点展开', async () => {
    seed([mainSession, forkSession]);
    render(<SessionList />);
    const mainRow = screen.getByText('主对话').closest('button');
    const badge = mainRow!.querySelector('[data-agent-fold-toggle]') as HTMLElement;
    expect(badge).not.toBeNull();
    expect(badge.getAttribute('data-folded')).toBe('false');
    // 点击符号：收起，且不切换会话
    fireEvent.click(badge);
    await waitFor(() => expect(screen.queryByText('编辑重发支线')).not.toBeInTheDocument());
    expect(switchSessionMock).not.toHaveBeenCalled();
    // 折叠态：符号上出现隐藏数徽标
    const foldedBadge = screen.getByText('主对话').closest('button')!
      .querySelector('[data-agent-fold-toggle]') as HTMLElement;
    expect(foldedBadge.getAttribute('data-folded')).toBe('true');
    expect(foldedBadge.querySelector('[data-agent-fold-count]')!.textContent).toBe('1');
    // 再点符号：展开，徽标消失
    fireEvent.click(foldedBadge);
    await waitFor(() => expect(screen.getByText('编辑重发支线')).toBeInTheDocument());
    const expandedBadge = screen.getByText('主对话').closest('button')!
      .querySelector('[data-agent-fold-toggle]') as HTMLElement;
    expect(expandedBadge.getAttribute('data-folded')).toBe('false');
    expect(expandedBadge.querySelector('[data-agent-fold-count]')).toBeNull();
  });

  it('点击行其它位置：只切换会话，不折叠子对话', async () => {
    seed([mainSession, forkSession]);
    render(<SessionList />);
    fireEvent.click(screen.getByText('主对话'));
    await waitFor(() => expect(switchSessionMock).toHaveBeenCalledWith('/tmp/agents/hana/sessions/main.jsonl'));
    // 行点击不折叠：子对话仍在
    expect(screen.getByText('编辑重发支线')).toBeInTheDocument();
  });

  it('子对话行与无子对话的行：助手符号不带折叠开关', () => {
    seed([mainSession, forkSession]);
    render(<SessionList />);
    const childRow = screen.getByText('编辑重发支线').closest('button');
    expect(childRow!.querySelector('[data-agent-fold-toggle]')).toBeNull();
  });
});
