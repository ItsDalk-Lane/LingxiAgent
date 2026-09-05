/**
 * v0.1.33 新建会话 desk 存档污染（机制c）表征测试（characterization，只诊断不修复）
 *
 * createNewSession 的清空 patch（session-actions.ts:1235-1237）只清三件套
 * （deskCurrentPath / deskFiles / deskJianContent），不清 deskBasePath /
 * deskWorkspaceMountId / deskTreeFilesByPath。随后 activateWorkspaceDesk（v0.1.33 起
 * createNewSession 必经，session-actions.ts:1240）先「快照当前 desk 再恢复」
 * （desk-actions.ts:394-455）：把这份清了一半的 desk 状态原样写回当前根
 * （= 继承的主工作台）的存档 workspaceDeskStateByRoot[root]。
 *
 * 后果（本测试坐实）：
 *  1. 主工作台存档的 deskFiles / deskCurrentPath / deskJianContent 被清空快照覆盖——
 *     用户下次回到该工作台时，上次打开的子目录与文件列表丢失；
 *  2. deskTreeFilesByPath 因从未被清，快照-恢复后原样保留旧（主）工作台的树缓存——
 *     新会话工作台激活失败/延迟期间，文件树仍显示旧工作台内容（串台面）。
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';

const mockLingxiFetch = vi.hoisted(() => vi.fn());

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: mockLingxiFetch,
}));

vi.mock('../../stores/agent-actions', () => ({
  clearChat: vi.fn(),
}));

function jsonResponse(body: unknown): Response {
  return { json: async () => body } as unknown as Response;
}

const MAIN_WORKSPACE_ROOT = '/workspace/main';

describe('v0.1.33 新建会话 desk 存档污染（characterization）', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    delete (globalThis as any).document;
    mockLingxiFetch.mockReset();
    mockLingxiFetch.mockImplementation(async (url: string) => {
      if (url.startsWith('/api/preferences/workspace-ui-state')) return jsonResponse({ state: null });
      if (url.startsWith('/api/desk/jian')) return jsonResponse({ content: null });
      return jsonResponse({});
    });
    (globalThis as any).window = {
      t: (key: string) => key,
      platform: {
        readFileSnapshot: vi.fn(async (filePath: string) => ({
          content: `content:${filePath}`,
          version: { mtimeMs: 1, size: 10, sha256: 'hash' },
        })),
        getFileUrl: vi.fn((filePath: string) => `file://${filePath}`),
      },
    };
    // 场景起点：主工作台 desk 处于「有内容」状态，且内存存档已有同形状条目。
    useStore.setState({
      serverPort: 62950,
      activeServerConnection: null,
      activeServerConnectionId: null,
      serverConnections: {},
      deskBasePath: MAIN_WORKSPACE_ROOT,
      deskWorkspaceMountId: null,
      deskWorkspaceLabel: null,
      deskCurrentPath: 'notes',
      deskFiles: [{ name: 'note.md', isDir: false }],
      deskJianContent: '主工作台笔记',
      deskTreeFilesByPath: {
        '': [{ name: 'notes', isDir: true }],
        'notes': [{ name: 'a.md', isDir: false }],
      },
      deskExpandedPaths: ['notes'],
      deskSelectedPath: 'notes/a.md',
      deskDirtyTreePaths: [],
      cwdSkills: [],
      jianDrawerOpen: false,
      rightWorkspaceTab: 'workspace',
      jianView: 'desk',
      previewOpen: false,
      previewItems: [],
      openTabs: [],
      activeTabId: null,
      previewReadingPositions: {},
      workspaceDeskStateByRoot: {
        [MAIN_WORKSPACE_ROOT]: {
          deskCurrentPath: 'notes',
          deskFiles: [{ name: 'note.md', isDir: false }],
          deskTreeFilesByPath: {
            '': [{ name: 'notes', isDir: true }],
            'notes': [{ name: 'a.md', isDir: false }],
          },
          deskExpandedPaths: ['notes'],
          deskSelectedPath: 'notes/a.md',
          deskJianContent: '主工作台笔记',
          cwdSkills: [],
          jianDrawerOpen: false,
          rightWorkspaceTab: 'workspace',
          jianView: 'desk',
          previewOpen: false,
          openTabs: [],
          activeTabId: null,
          previewReadingPositions: {},
        },
      },
      selectedFolder: MAIN_WORKSPACE_ROOT,
      selectedWorkspaceMountId: null,
      selectedWorkspaceLabel: null,
      studioWorkspaces: [],
      homeFolder: '/fallback-home',
      workspaceFolders: [],
      pendingNewSession: false,
      currentSessionPath: '/session/old-main.jsonl',
      currentAgentId: 'hana',
      selectedAgentId: null,
    } as never);
  });

  it('characterization: KNOWN DEFECT — createNewSession 的半清空 desk 被 activateWorkspaceDesk 快照写回主工作台存档', async () => {
    // createNewSession 的清空 patch（session-actions.ts:1235-1237）：
    // 只清三件套；deskBasePath / deskWorkspaceMountId / deskTreeFilesByPath 不动。
    useStore.setState({
      deskCurrentPath: '',
      deskFiles: [],
      deskJianContent: null,
    } as never);

    const { activateWorkspaceDesk } = await import('../../stores/desk-actions');

    // v0.1.33 继承路径：同一主工作台根再次激活（createNewSession → activateWorkspaceDesk）
    await activateWorkspaceDesk(MAIN_WORKSPACE_ROOT, { reload: false });

    const archive = useStore.getState().workspaceDeskStateByRoot[MAIN_WORKSPACE_ROOT];

    // 机制c污染坐实：主工作台存档被「清了一半」的快照覆盖
    expect(archive.deskFiles).toEqual([]);
    expect(archive.deskCurrentPath).toBe('');
    expect(archive.deskJianContent).toBeNull();

    // desk 内存仍指向旧（主）工作台；树缓存因从未被清而原样保留（串台面）
    expect(useStore.getState().deskBasePath).toBe(MAIN_WORKSPACE_ROOT);
    expect(useStore.getState().deskTreeFilesByPath).toEqual({
      '': [{ name: 'notes', isDir: true }],
      'notes': [{ name: 'a.md', isDir: false }],
    });
  });
});
