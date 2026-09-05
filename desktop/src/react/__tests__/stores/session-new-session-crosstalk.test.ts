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

const MAIN_WORKSPACE_SESSION_PATH = '/session/main-workspace.jsonl';
const OLD_SESSION_PATH = '/session/old-main.jsonl';
const NEW_SESSION_PATH = '/session/new-v033.jsonl';

describe('v0.1.33 新建会话串台（characterization）', () => {
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
    // 与真实 activateWorkspaceDesk（desk-actions.ts:380-495）同契约的最小镜像：
    // 先把当前 desk（含 deskTreeFilesByPath）快照进当前根的存档，再按目标根恢复。
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
          cwdSkillsOpen: false,
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

  it('characterization: KNOWN DEFECT — 无会话态下 loadSessions 把视图强切到 sessions[0]（主工作台会话）并加载其记录', async () => {
    // 无会话态 = 三标志同时为「无」：
    //   currentSessionPath=null + pendingNewSession=false + pendingSessionSwitchPath=null。
    // 真实到达点：冷启动初始形状（session-slice.ts:201-206 默认值）、
    // 归档当前会话后的中间形状（session-actions.ts:1380 清空后、:1389 兜底切换前）。
    // （新建会话流程内部是否可达该状态 → 见 PROGRESS.md 静态枚举排除清单。）
    Object.assign(mockState, {
      currentSessionPath: null,
      currentSessionId: null,
      pendingNewSession: false,
      pendingSessionSwitchPath: null,
      sessions: [],
    });
    mockFetch.mockImplementation(async (url: string) => {
      if (url === '/api/sessions') {
        // 服务端列表第一个 = 最近修改 = 主工作台会话
        return jsonResponse([
          {
            path: MAIN_WORKSPACE_SESSION_PATH,
            sessionId: 'sess_main_workspace',
            title: '主工作台',
            firstMessage: '主工作台的第一条',
            modified: '2026-09-04T01:00:00.000Z',
            messageCount: 3,
          },
          {
            path: OLD_SESSION_PATH,
            sessionId: 'sess_old_main',
            modified: '2026-09-03T01:00:00.000Z',
            messageCount: 1,
          },
        ]);
      }
      if (url === '/api/health') return jsonResponse({ ok: true });
      if (String(url).startsWith('/api/sessions/messages')) {
        return jsonResponse({
          messages: [{ role: 'user', content: '主工作台的记录' }],
          blocks: [],
          todos: [],
          hasMore: false,
        });
      }
      if (url === '/api/sessions/switch') {
        return jsonResponse({
          ok: true,
          path: MAIN_WORKSPACE_SESSION_PATH,
          sessionId: 'sess_main_workspace',
          cwd: '/workspace/main',
          workspaceFolders: [],
          memoryEnabled: true,
          permissionMode: 'ask',
        });
      }
      throw new Error(`unexpected fetch: ${url}`);
    });

    await loadSessions();

    // 强切拉力：视图被拉到 sessions[0] = 主工作台会话
    expect(mockState.currentSessionPath).toBe(MAIN_WORKSPACE_SESSION_PATH);
    expect(mockState.currentSessionId).toBe('sess_main_workspace');
    expect(mockState.welcomeVisible).toBe(false);
    expect(mockState.pendingNewSession).toBe(false);
    // 主工作台的历史记录被加载进当前视图缓存
    const cache = (mockState.chatSessions as Record<string, { items: unknown[] } | undefined>)[MAIN_WORKSPACE_SESSION_PATH];
    expect(cache).toBeTruthy();
    expect(cache?.items).toHaveLength(1);
  });

  it('characterization: KNOWN DEFECT — createNewSession/ensureSession 全程结束后 desk 内存（base/mountId/tree 缓存）仍指向旧（主）工作台（机制c半清空）', async () => {
    // 场景：旧会话绑定 studio mount 主工作台，desk 树缓存有旧树
    Object.assign(mockState, {
      agents: [{
        id: 'hana', name: 'Hana', isPrimary: true, effectiveHomeFolder: '/workspace/Primary',
      }],
      currentAgentId: 'hana',
      currentSessionId: 'sess_old_main',
      currentSessionPath: OLD_SESSION_PATH,
      sessions: [{
        sessionId: 'sess_old_main',
        path: OLD_SESSION_PATH,
        agentId: 'hana',
        cwd: '/resolved/studio/main',
        workspaceMountId: 'mount_main',
        workspaceLabel: '主工作台',
      }],
      deskBasePath: 'studio:mount_main',
      deskWorkspaceMountId: 'mount_main',
      deskWorkspaceLabel: '主工作台',
      deskCurrentPath: 'notes',
      deskFiles: [{ name: 'note.md' }],
      deskJianContent: '旧笔记',
      deskTreeFilesByPath: { 'notes': [{ name: 'a.md' }] },
      workspaceDeskStateByRoot: {
        'studio:mount_main': {
          deskCurrentPath: 'notes',
          deskFiles: [{ name: 'note.md' }],
          deskTreeFilesByPath: { 'notes': [{ name: 'a.md' }] },
          deskJianContent: '旧笔记',
          cwdSkills: [],
          cwdSkillsOpen: false,
          previewOpen: false,
          openTabs: [],
          activeTabId: null,
        },
      },
    });

    // createNewSession 阶段（permission + thinking 默认值）
    mockFetch.mockImplementation(async (url: string) => {
      if (url === '/api/preferences/session-permission-default') return jsonResponse({ permissionMode: 'ask' });
      if (url === '/api/session-thinking-level?pendingNewSession=1') return jsonResponse({ thinkingLevel: 'medium' });
      throw new Error(`unexpected fetch during createNewSession: ${url}`);
    });

    await createNewSession();

    // createNewSession 的清空 patch（session-actions.ts:1235-1237）只清三件套；
    // 继承同一主工作台 → activateWorkspaceDesk('/resolved/studio/main', {mountId:'mount_main'})
    // 快照-恢复同 key：deskBasePath / deskWorkspaceMountId / deskTreeFilesByPath 原样保留。
    expect(mockState.deskWorkspaceMountId).toBe('mount_main');
    expect(mockState.deskBasePath).toBe('studio:mount_main');
    expect(mockState.deskTreeFilesByPath).toEqual({ 'notes': [{ name: 'a.md' }] });
    // 三件套已清（新会话草稿不显示旧文件列表/子目录/简内容）
    expect(mockState.deskCurrentPath).toBe('');
    expect(mockState.deskFiles).toEqual([]);
    expect(mockState.deskJianContent).toBeNull();

    // ensureSession 阶段：建会话 + 切换（继承同一工作台）
    mockFetch.mockReset();
    mockFetch.mockImplementation(async (url: string) => {
      if (url === '/api/sessions/new-detached') {
        return jsonResponse({
          ok: true,
          path: NEW_SESSION_PATH,
          sessionId: 'sess_new_v033',
          agentId: 'hana',
          cwd: '/resolved/studio/main',
          workspaceFolders: [],
        });
      }
      if (url === '/api/sessions/switch') {
        return jsonResponse({
          ok: true,
          path: NEW_SESSION_PATH,
          sessionId: 'sess_new_v033',
          agentId: 'hana',
          cwd: '/resolved/studio/main',
          workspaceMountId: 'mount_main',
          workspaceLabel: '主工作台',
          workspaceFolders: [],
          memoryEnabled: true,
          permissionMode: 'ask',
        });
      }
      if (String(url).startsWith('/api/sessions/messages')) {
        return jsonResponse({ messages: [], blocks: [], todos: [], hasMore: false });
      }
      throw new Error(`unexpected fetch during ensureSession: ${url}`);
    });

    await ensureSession();

    // 全流程结束：会话身份已是新会话，但 desk 内存三字段仍指向旧（主）工作台。
    // 会话身份/desk 缓存两套事实无事务绑定——desk 侧从未随新会话重置（机制c）。
    expect(mockState.currentSessionPath).toBe(NEW_SESSION_PATH);
    expect(mockState.deskWorkspaceMountId).toBe('mount_main');
    expect(mockState.deskBasePath).toBe('studio:mount_main');
    expect(mockState.deskTreeFilesByPath).toEqual({ 'notes': [{ name: 'a.md' }] });
  });
});
