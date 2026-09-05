/**
 * v0.1.33 新建会话工作台继承回归测试（2026-09-05 修复后锁定）
 *
 * 修复契约：createNewSession 在「无当前会话」（切换工作台后的草稿态 / 冷启动恢复）时，
 * 继承「当前显示的工作台」——草稿选择（applyFolder/applyStudioWorkspace 写入的
 * selectedFolder / selectedWorkspaceMountId）优先，其次 desk 已激活身份；
 * 仅当两者皆空才落 Primary Agent 工作台（设置页「新建对话默认工作台」语义）。
 *
 * 修复前的缺陷（用户实测）：切到非默认工作台后点「新建聊天」，草稿被拽回 Primary
 * 工作台，左栏列表作用域（resolveWorkspaceScope 草稿态读 selected*）跟着换轨成
 * 默认工作台的记录；Default 工作台下表现同为「列表显示设置目录的记录」。
 */

/**
 * v0.1.33 新建会话「串台成主工作台」机制表征测试（characterization，只诊断不修复）
 *
 * 两个半边：
 *  A. 机制(b)——loadSessions 的强切拉力（session-actions.ts:626-635）：
 *     「无当前会话 + 非草稿 + 无切换中」时 loadSessions 会把视图强切到 sessions[0]
 *     （服务端列表第一个 = 最近修改的会话，通常是主工作台会话），并把其记录拉进首屏。
 *     本测试直接构造该状态（冷启动初始形状 session-slice.ts:201-206 / 归档当前会话后
 *     session-actions.ts:1380 的中间形状），证明拉力本身及其效果。
 *     该状态在「新建会话流程」内部是否可达，见交付的静态枚举清单（PROGRESS.md）。
 *  B. 机制(c)文件树半边——createNewSession 的 desk 半清空（session-actions.ts:1235-1237
 *     只清 deskCurrentPath/deskFiles/deskJianContent），deskBasePath/deskWorkspaceMountId/
 *     deskTreeFilesByPath 从不重置；继承同工作台激活后三者仍指向旧（主）工作台。
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { InlineErrorEntry } from '../../stores/streaming-slice';

type MockState = Record<string, unknown>;

const deskActionMocks = vi.hoisted(() => ({
  loadDeskFiles: vi.fn(),
  activateWorkspaceDesk: vi.fn(),
}));

const streamResumeMocks = vi.hoisted(() => ({
  requestStreamResume: vi.fn(),
}));

const websocketMocks = vi.hoisted(() => ({
  requestTerminalSnapshotForCurrentSession: vi.fn(() => true),
}));

const mockState: MockState = {};
const initialStateFactory = (): MockState => ({
  currentSessionPath: null,
  currentSessionId: null,
  sessionLocatorsById: {} as Record<string, { path: string | null }>,
  pendingSessionSwitchPath: null,
  pendingNewSession: false,
  pendingDraftId: null,
  pendingProjectId: null,
  pendingNewSessionThinkingLevel: null,
  pendingNewSessionPermissionMode: null,
  sessionPermissionMode: 'ask',
  sessions: [] as Array<{ path: string }>,
  chatSessions: {} as Record<string, unknown>,
  sessionRegistryFilesByPath: {} as Record<string, unknown>,
  sessionModelsByPath: {} as Record<string, unknown>,
  _loadMessagesVersion: {} as Record<string, number>,
  _sessionFilesFlightByPath: {} as Record<string, { version: number; resetSeen: boolean; upserts: Record<string, unknown>[] }>,
  scrollPositions: {} as Record<string, number>,
  todosLiveVersionBySession: {} as Record<string, number>,
  todosBySession: {} as Record<string, unknown>,
  sessionStreams: {} as Record<string, unknown>,
  attachedFiles: [],
  attachedFilesBySession: {} as Record<string, unknown>,
  drafts: {} as Record<string, string>,
  draftDocs: {} as Record<string, unknown>,
  setDraft: vi.fn(),
  clearDraft: vi.fn(),
  streamingSessions: [] as string[],
  unreadOutputSessionPaths: [] as string[],
  capabilityRefreshingSessions: [] as string[],
  inlineErrors: {} as Record<string, InlineErrorEntry | null>,
  addToast: vi.fn(),
  activePanel: null,
  currentTab: 'chat',
  settingsModal: { open: false, activeTab: 'agent' },
  mediaViewer: null,
  skillViewerData: null,
  channelCreateOverlayVisible: false,
  computerOverlayBySession: {} as Record<string, unknown>,
  agents: [] as unknown[],
  currentAgentId: null,
  agentName: '',
  agentYuan: 'lingxi',
  agentAvatarUrl: null,
  memoryEnabled: true,
  browserBySession: {} as Record<string, unknown>,
  welcomeVisible: false,
  deskContextAttached: false,
  docContextAttached: false,
  deskBasePath: '',
  deskCurrentPath: '',
  deskFiles: [] as unknown[],
  deskJianContent: null,
  deskTreeFilesByPath: {} as Record<string, unknown>,
  workspaceDeskStateByRoot: {} as Record<string, unknown>,
  homeFolder: null,
  selectedFolder: null,
  selectedWorkspaceMountId: null,
  selectedWorkspaceLabel: null,
  deskWorkspaceMountId: null,
  deskWorkspaceLabel: null,
  studioWorkspaces: [],
  workspaceFolders: [] as string[],
  cwdHistory: [] as string[],
  selectedAgentId: null,
  thinkingLevel: 'medium',
  metaRecovery: null as unknown,
});

const dispatchedEvents: CustomEvent[] = [];

vi.mock('../../stores', () => ({
  useStore: {
    getState: () => mockState,
    setState: (patch: MockState | ((s: MockState) => MockState)) => {
      const next = typeof patch === 'function' ? patch(mockState) : patch;
      Object.assign(mockState, next);
    },
  },
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: vi.fn(),
  lingxiUrl: (p: string) => p,
}));

vi.mock('../../utils/history-builder', () => ({
  buildItemsFromHistory: (data: { messages?: unknown[] }) => (data.messages || []).map((m, i) => ({
    type: 'message' as const,
    data: { id: String(i), ...(m as object) },
  })),
}));

vi.mock('../../utils/todo-compat', () => ({
  migrateLegacyTodos: (x: { todos: unknown[] }) => x.todos,
}));

vi.mock('../../utils/ui-helpers', () => ({
  loadModels: vi.fn(),
}));

vi.mock('./agent-actions', () => ({
  loadAvatars: vi.fn(),
  clearChat: vi.fn(),
}));

vi.mock('../../stores/agent-actions', () => ({
  loadAvatars: vi.fn(),
  clearChat: vi.fn(),
}));

vi.mock('../../stores/desk-actions', () => ({
  loadDeskFiles: deskActionMocks.loadDeskFiles,
  activateWorkspaceDesk: deskActionMocks.activateWorkspaceDesk,
}));

vi.mock('../../stores/create-keyed-slice', () => ({
  updateKeyed: vi.fn(),
}));

vi.mock('../../stores/stream-invalidator', () => ({
  snapshotStreamBuffer: vi.fn(),
  invalidateStreamBuffer: vi.fn(),
  registerStreamBufferInvalidator: vi.fn(),
  registerStreamBufferSnapshot: vi.fn(),
}));

vi.mock('../../utils/markdown', () => ({
  renderMarkdown: (s: string) => `<p>${s}</p>`,
}));

vi.mock('../../services/websocket', () => ({
  getWebSocket: () => null,
  requestTerminalSnapshotForCurrentSession: websocketMocks.requestTerminalSnapshotForCurrentSession,
}));

vi.mock('../../services/stream-resume', () => ({
  requestStreamResume: streamResumeMocks.requestStreamResume,
}));

// Stub window.dispatchEvent / CustomEvent for jsdom-less runs
if (typeof window === 'undefined') {
  (globalThis as any).window = {
    dispatchEvent: (e: CustomEvent) => { dispatchedEvents.push(e); return true; },
  };
  (globalThis as any).CustomEvent = class {
    type: string;
    detail: unknown;
    constructor(type: string, init?: { detail: unknown }) {
      this.type = type;
      this.detail = init?.detail;
    }
  };
} else {
  window.dispatchEvent = ((e: Event) => {
    dispatchedEvents.push(e as CustomEvent);
    return true;
  }) as typeof window.dispatchEvent;
}

// Stub store methods used by loadMessages / switchSession
function installStoreMethods() {
  const s = mockState as MockState;
  s.initSession = vi.fn((path: string, items: unknown[], hasMore: boolean, revision?: string | null) => {
    const chat = mockState.chatSessions as Record<string, unknown>;
    chat[path] = { items, hasMore, loadingMore: false, revision: revision ?? null };
  });
  s.bumpLoadMessagesVersion = vi.fn((path: string) => {
    const versions = mockState._loadMessagesVersion as Record<string, number>;
    const next = (versions[path] ?? 0) + 1;
    versions[path] = next;
    return next;
  });
  s.updateSessionModel = vi.fn((path: string, model: unknown) => {
    const models = mockState.sessionModelsByPath as Record<string, unknown>;
    models[path] = model;
  });
  s.clearSession = vi.fn((path: string) => {
    delete (mockState.chatSessions as Record<string, unknown>)[path];
    delete (mockState.sessionRegistryFilesByPath as Record<string, unknown>)[path];
    delete (mockState.sessionModelsByPath as Record<string, unknown>)[path];
    delete (mockState._loadMessagesVersion as Record<string, number>)[path];
    delete (mockState.scrollPositions as Record<string, number>)[path];
    delete (mockState._sessionFilesFlightByPath as Record<string, unknown>)[path];
  });
  s.clearTerminals = vi.fn();
  s.setSessionRegistryFiles = vi.fn((path: string, files: unknown[]) => {
    const bySession = mockState.sessionRegistryFilesByPath as Record<string, unknown>;
    bySession[path] = files;
  });
  s.upsertSessionRegistryFile = vi.fn((path: string, file: Record<string, unknown>) => {
    const bySession = mockState.sessionRegistryFilesByPath as Record<string, Record<string, unknown>[]>;
    const files = bySession[path] || [];
    bySession[path] = [...files, file];
    const flightByPath = mockState._sessionFilesFlightByPath as Record<string, { version: number; resetSeen: boolean; upserts: Record<string, unknown>[] }>;
    const flight = flightByPath[path];
    if (flight) flight.upserts = [...flight.upserts, file];
  });
  s.beginSessionFilesFlight = vi.fn((path: string, version: number) => {
    const flightByPath = mockState._sessionFilesFlightByPath as Record<string, { version: number; resetSeen: boolean; upserts: Record<string, unknown>[] }>;
    flightByPath[path] = { version, resetSeen: false, upserts: [] };
  });
  s.consumeSessionFilesFlight = vi.fn((path: string, version: number) => {
    const flightByPath = mockState._sessionFilesFlightByPath as Record<string, { version: number; resetSeen: boolean; upserts: Record<string, unknown>[] }>;
    const flight = flightByPath[path];
    if (!flight || flight.version !== version) return null;
    delete flightByPath[path];
    return { resetSeen: flight.resetSeen, upserts: flight.upserts };
  });
  s.applyBranchResetSessionFiles = vi.fn((path: string, files: unknown[] | null) => {
    if (Array.isArray(files)) {
      const bySession = mockState.sessionRegistryFilesByPath as Record<string, unknown>;
      bySession[path] = files;
    }
    const flightByPath = mockState._sessionFilesFlightByPath as Record<string, { version: number; resetSeen: boolean; upserts: Record<string, unknown>[] }>;
    const flight = flightByPath[path];
    if (flight) flight.resetSeen = true;
  });
  s.setSessionTodosForPath = vi.fn((path: string, todos: unknown[]) => {
    const bySession = mockState.todosBySession as Record<string, unknown>;
    bySession[path] = todos;
  });
  s.bumpTodosLiveVersion = vi.fn((path: string) => {
    const versions = mockState.todosLiveVersionBySession as Record<string, number>;
    versions[path] = (versions[path] ?? 0) + 1;
  });
  s.setInlineError = vi.fn((path: string, error: string | InlineErrorEntry) => {
    const inlineErrors = mockState.inlineErrors as Record<string, InlineErrorEntry | null>;
    inlineErrors[path] = typeof error === 'string'
      ? { text: error, detail: null, code: null }
      : error;
  });
  s.appendItem = vi.fn((path: string, item: unknown) => {
    const chat = mockState.chatSessions as Record<string, { items: unknown[] }>;
    const entry = chat[path];
    if (entry) entry.items.push(item);
  });
  s.clearQuotedSelection = vi.fn();
  s.setActivePanel = vi.fn((v: unknown) => { mockState.activePanel = v; });
  s.requestInputFocus = vi.fn();
  s.setThinkingLevel = vi.fn((level: string) => { mockState.thinkingLevel = level; });
  s.setPendingNewSessionThinkingLevel = vi.fn((level: string | null) => { mockState.pendingNewSessionThinkingLevel = level; });
  s.setSessionPermissionMode = vi.fn((mode: string) => {
    mockState.sessionPermissionMode = mode;
    if (mockState.pendingNewSession === true) {
      mockState.pendingNewSessionPermissionMode = mode;
    }
  });
  s.setPendingNewSessionPermissionMode = vi.fn((mode: string | null) => { mockState.pendingNewSessionPermissionMode = mode; });
  s.setSessionCapabilityRefreshing = vi.fn((path: string, refreshing: boolean) => {
    const list = mockState.capabilityRefreshingSessions as string[];
    mockState.capabilityRefreshingSessions = refreshing
      ? (list.includes(path) ? list : [...list, path])
      : list.filter((p) => p !== path);
  });
  s.setDeskBasePath = vi.fn((path: string) => { mockState.deskBasePath = path; });
  s.setDeskCurrentPath = vi.fn((path: string) => { mockState.deskCurrentPath = path; });
  s.setDeskFiles = vi.fn((files: unknown[]) => { mockState.deskFiles = files; });
  s.setDeskJianContent = vi.fn((content: string | null) => { mockState.deskJianContent = content; });
  s.clearStaleMessageLocate = vi.fn();
  s.setSessionMetaRecovery = vi.fn((status: unknown) => { mockState.metaRecovery = status; });
}

import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { clearMessageLiveVersion } from '../../stores/message-live-version';
import { createNewSession, ensureSession, loadSessions } from '../../stores/session-actions';
import { snapshotStreamBuffer } from '../../stores/stream-invalidator';

const mockFetch = vi.mocked(lingxiFetch);
const mockSnapshot = vi.mocked(snapshotStreamBuffer);

function jsonResponse(body: unknown, ok = true): Response {
  return { ok, json: async () => body } as unknown as Response;
}

const OLD_SESSION_PATH = '/session/old-main.jsonl';
const NEW_SESSION_PATH = '/session/new-v033-ws.jsonl';

describe('v0.1.33 新建会话工作台继承（regression: FIXED）', () => {
  beforeEach(() => {
    Object.keys(mockState).forEach(k => delete mockState[k]);
    Object.assign(mockState, initialStateFactory());
    Object.assign(mockState, { workspaceDeskStateByRoot: {} as Record<string, unknown> });
    (globalThis.window as unknown as { hana?: unknown }).hana = {};
    installStoreMethods();
    mockFetch.mockReset();
    streamResumeMocks.requestStreamResume.mockReset();
    websocketMocks.requestTerminalSnapshotForCurrentSession.mockClear();
    deskActionMocks.activateWorkspaceDesk.mockReset();
    // 与真实 activateWorkspaceDesk（desk-actions.ts:380-495）同契约的最小镜像。
    deskActionMocks.activateWorkspaceDesk.mockImplementation(async (root?: string | null, options?: { mountId?: string | null; label?: string | null }) => {
      const mountId = options?.mountId || null;
      const normalized = mountId ? `studio:${mountId}` : (root || '');
      const currentRoot = (mockState.deskWorkspaceMountId as string | null)
        ? `studio:${mockState.deskWorkspaceMountId as string}`
        : ((mockState.deskBasePath as string) || '');
      const states = mockState.workspaceDeskStateByRoot as Record<string, any>;
      if (currentRoot) {
        states[currentRoot] = {
          ...(states[currentRoot] || {}),
          deskCurrentPath: (mockState.deskCurrentPath as string) || '',
          deskFiles: mockState.deskFiles,
          deskTreeFilesByPath: { ...(mockState.deskTreeFilesByPath as Record<string, unknown>) },
          deskJianContent: mockState.deskJianContent,
          cwdSkills: [],
          previewOpen: false,
          openTabs: [],
          activeTabId: null,
        };
      }
      if (!normalized) {
        mockState.deskBasePath = '';
        mockState.deskWorkspaceMountId = null;
        mockState.deskWorkspaceLabel = null;
        mockState.deskCurrentPath = '';
        mockState.deskFiles = [];
        mockState.deskTreeFilesByPath = {};
        mockState.deskJianContent = null;
        return;
      }
      const saved = states[normalized] || null;
      mockState.deskBasePath = normalized;
      mockState.deskWorkspaceMountId = mountId;
      mockState.deskWorkspaceLabel = options?.label || null;
      mockState.deskCurrentPath = '';
      mockState.deskFiles = [];
      mockState.deskTreeFilesByPath = saved?.deskTreeFilesByPath || {};
      mockState.deskJianContent = null;
      deskActionMocks.loadDeskFiles('', mountId ? null : normalized, mountId);
    });
    mockSnapshot.mockReset();
    mockSnapshot.mockReturnValue(null);
    clearMessageLiveVersion();
    dispatchedEvents.length = 0;
  });

  function mockCreateNewSessionFetches() {
    mockFetch.mockImplementation(async (url: string) => {
      if (url === '/api/preferences/session-permission-default') return jsonResponse({ permissionMode: 'ask' });
      if (url === '/api/session-thinking-level?pendingNewSession=1') return jsonResponse({ thinkingLevel: 'medium' });
      throw new Error(`unexpected fetch during createNewSession: ${url}`);
    });
  }

  it('regression: FIXED — 无当前会话 + 草稿 mount 选择（applyStudioWorkspace 后）：新建聊天留在该 mount，不拽回 Primary', async () => {
    Object.assign(mockState, {
      agents: [{ id: 'hana', name: 'Hana', isPrimary: true, effectiveHomeFolder: '/workspace/Primary' }],
      currentAgentId: 'hana',
      // 用户在聊天界面把工作台切到 mount_b 后的草稿态（applyStudioWorkspace 写入）
      pendingNewSession: true,
      currentSessionPath: null,
      selectedWorkspaceMountId: 'mount_b',
      selectedWorkspaceLabel: '工作台B',
      selectedFolder: null,
      deskBasePath: 'studio:mount_b',
      deskWorkspaceMountId: 'mount_b',
      deskWorkspaceLabel: '工作台B',
    });
    mockCreateNewSessionFetches();

    await createNewSession();

    expect(mockState.selectedWorkspaceMountId).toBe('mount_b');
    expect(mockState.selectedWorkspaceLabel).toBe('工作台B');
    expect(mockState.selectedFolder).toBeNull();
    expect(deskActionMocks.activateWorkspaceDesk).toHaveBeenCalledWith(null, { mountId: 'mount_b', label: '工作台B' });
    expect(deskActionMocks.activateWorkspaceDesk).not.toHaveBeenCalledWith('/workspace/Primary', expect.anything());
    expect(mockState.deskBasePath).toBe('studio:mount_b');
    expect(mockState.deskWorkspaceMountId).toBe('mount_b');
  });

  it('regression: FIXED — 无当前会话 + 草稿 folder 选择（applyFolder 后）：新建聊天留在该目录', async () => {
    Object.assign(mockState, {
      agents: [{ id: 'hana', name: 'Hana', isPrimary: true, effectiveHomeFolder: '/workspace/Primary' }],
      currentAgentId: 'hana',
      pendingNewSession: true,
      currentSessionPath: null,
      selectedFolder: '/workspace/B',
      selectedWorkspaceMountId: null,
      deskBasePath: '/workspace/B',
      deskWorkspaceMountId: null,
    });
    mockCreateNewSessionFetches();

    await createNewSession();

    expect(mockState.selectedFolder).toBe('/workspace/B');
    expect(mockState.selectedWorkspaceMountId).toBeNull();
    expect(deskActionMocks.activateWorkspaceDesk).toHaveBeenCalledWith('/workspace/B', { mountId: null, label: null });
    expect(deskActionMocks.activateWorkspaceDesk).not.toHaveBeenCalledWith('/workspace/Primary', expect.anything());
    expect(mockState.deskBasePath).toBe('/workspace/B');
  });

  it('regression: FIXED — 无当前会话 + 仅 desk 身份（冷启动恢复窗口）：新建聊天留在 desk 显示的目录', async () => {
    Object.assign(mockState, {
      agents: [{ id: 'hana', name: 'Hana', isPrimary: true, effectiveHomeFolder: '/workspace/Primary' }],
      currentAgentId: 'hana',
      pendingNewSession: false,
      currentSessionPath: null,
      selectedFolder: null,
      selectedWorkspaceMountId: null,
      deskBasePath: '/workspace/B',
      deskWorkspaceMountId: null,
    });
    mockCreateNewSessionFetches();

    await createNewSession();

    expect(mockState.selectedFolder).toBe('/workspace/B');
    expect(deskActionMocks.activateWorkspaceDesk).toHaveBeenCalledWith('/workspace/B', { mountId: null, label: null });
  });

  it('regression: FIXED — 无当前会话 + 无任何工作台：仍落 Primary Agent 工作台（设置页默认语义保留）', async () => {
    Object.assign(mockState, {
      agents: [{ id: 'hana', name: 'Hana', isPrimary: true, effectiveHomeFolder: '/workspace/Primary' }],
      currentAgentId: 'mio',
      currentSessionPath: null,
      selectedFolder: null,
      selectedWorkspaceMountId: null,
      deskBasePath: '',
      deskWorkspaceMountId: null,
    });
    mockCreateNewSessionFetches();

    await createNewSession();

    expect(mockState.selectedFolder).toBe('/workspace/Primary');
    expect(deskActionMocks.activateWorkspaceDesk).toHaveBeenCalledWith('/workspace/Primary', { mountId: null, label: null });
  });

  it('regression: FIXED — 继承的本地目录归一为规范形态：Windows 反斜杠 cwd 落 selectedFolder 前转正斜杠', async () => {
    // 服务端投影的 cwd 在 Windows 上是反斜杠原生路径；继承链若只 trim 不归一，
    // selectedFolder 会携带反斜杠形态，所有按 '/' 取目录名的显示位退化为整条路径。
    Object.assign(mockState, {
      agents: [{ id: 'hana', name: 'Hana', isPrimary: true, effectiveHomeFolder: '/workspace/Primary' }],
      currentAgentId: 'hana',
      currentSessionId: 'sess_win_cwd',
      currentSessionPath: OLD_SESSION_PATH,
      sessions: [{
        sessionId: 'sess_win_cwd',
        path: OLD_SESSION_PATH,
        agentId: 'hana',
        cwd: 'C:\\Users\\lts_D\\Desktop\\Project\\nest-drama',
      }],
      deskBasePath: 'C:\\Users\\lts_D\\Desktop\\Project\\nest-drama',
      deskWorkspaceMountId: null,
    });
    mockCreateNewSessionFetches();

    await createNewSession();

    expect(mockState.selectedFolder).toBe('C:/Users/lts_D/Desktop/Project/nest-drama');
    expect(deskActionMocks.activateWorkspaceDesk).toHaveBeenCalledWith(
      'C:/Users/lts_D/Desktop/Project/nest-drama',
      { mountId: null, label: null },
    );
  });

  it('regression: FIXED — reconcile 自愈链：发送后 loadSessions 发现列表 revision 前进，自动补拉 hydrate 新会话缓存', async () => {
    // 场景：旧会话 → 新建会话并发出首条消息（WS 事件全程缺席）
    Object.assign(mockState, {
      agents: [{ id: 'hana', name: 'Hana', isPrimary: true, effectiveHomeFolder: '/workspace/Primary' }],
      currentAgentId: 'hana',
      currentSessionId: 'sess_old_ws',
      currentSessionPath: OLD_SESSION_PATH,
      sessions: [{ sessionId: 'sess_old_ws', path: OLD_SESSION_PATH, agentId: 'hana', cwd: '/workspace/B' }],
      deskBasePath: '/workspace/B',
      deskWorkspaceMountId: null,
    });
    mockCreateNewSessionFetches();
    await createNewSession();

    let messagesCallCount = 0;
    mockFetch.mockReset();
    mockFetch.mockImplementation(async (url: string) => {
      if (url === '/api/sessions/new-detached') {
        return jsonResponse({
          ok: true,
          path: NEW_SESSION_PATH,
          sessionId: 'sess_new_ws',
          agentId: 'hana',
          cwd: '/workspace/B',
          workspaceFolders: [],
        });
      }
      if (url === '/api/sessions/switch') {
        return jsonResponse({
          ok: true,
          path: NEW_SESSION_PATH,
          sessionId: 'sess_new_ws',
          agentId: 'hana',
          cwd: '/workspace/B',
          workspaceFolders: [],
          memoryEnabled: true,
          permissionMode: 'ask',
        });
      }
      if (String(url).startsWith('/api/sessions/messages')) {
        messagesCallCount += 1;
        // 首次 hydrate：会话刚建、首条消息尚未落盘（revision r1）
        if (messagesCallCount === 1) {
          return jsonResponse({ messages: [], blocks: [], todos: [], hasMore: false, revision: 'r1' });
        }
        // reconcile 补拉：消息已落盘（revision r2）
        return jsonResponse({ messages: [{ role: 'user', content: '第一条消息' }], blocks: [], todos: [], hasMore: false, revision: 'r2' });
      }
      if (url === '/api/sessions') {
        return jsonResponse([
          { path: NEW_SESSION_PATH, sessionId: 'sess_new_ws', modified: '2026-09-05T01:00:00.000Z', revision: 'r2', messageCount: 1 },
        ]);
      }
      if (url === '/api/health') return jsonResponse({ ok: true });
      throw new Error(`unexpected fetch: ${url}`);
    });

    await ensureSession();

    // 建会话完成：历史已真实加载（空历史 stamp revision r1——不再种无 revision 的空缓存）
    const cacheAfterCreate = (mockState.chatSessions as Record<string, { items: unknown[]; revision: string | null }>)[NEW_SESSION_PATH];
    expect(cacheAfterCreate).toBeTruthy();
    expect(cacheAfterCreate?.items).toEqual([]);
    expect(cacheAfterCreate?.revision).toBe('r1');
    expect(mockState.currentSessionPath).toBe(NEW_SESSION_PATH);

    // InputArea 发送后的 loadSessions：列表投影 revision r2 前进 → reconcile 自动补拉
    await loadSessions();
    await vi.waitFor(() => {
      const cache = (mockState.chatSessions as Record<string, { items: unknown[]; revision: string | null }>)[NEW_SESSION_PATH];
      expect(cache?.revision).toBe('r2');
      expect(cache?.items).toHaveLength(1);
    });
    expect(messagesCallCount).toBe(2);
  });
});
