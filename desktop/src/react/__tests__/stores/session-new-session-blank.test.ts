/**
 * v0.1.33 新建会话「首屏空白」回归测试（2026-09-05 修复后锁定）
 *
 * 缺陷机制（修复前，见 PROGRESS.md 2026-09-04 任务1）：
 *  stageDetachedSessionForActivation 曾用 initSession(path, [], false) 预种空缓存，
 *  switchSession 的 hasData 判据因此为真跳过 loadMessages（session-actions.ts 原 :864-868），
 *  首屏内容完全依赖 WS 事件——事件被入口闸门丢弃/迟到即整片空白。
 *
 * 修复后契约（本测试锁定）：
 *  1. 不再预种空缓存：ensureSession → switchSession 走 !hasData → loadMessages 真正拉取
 *     /api/sessions/messages（即使 WS 全程无事件，服务端已有的历史也会被加载）；
 *  2. 历史加载后缓存 hydrate、revision stamp，welcomeVisible=false、会话身份正确；
 *  3. WS 先到时（缓存已有真实内容）hasData 为真跳过重复加载——由 session_user_message
 *     处理器负责 initSession（ws-message-handler.ts），此处不覆盖。
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
import { loadDeskFiles } from '../../stores/desk-actions';
import { clearMessageLiveVersion } from '../../stores/message-live-version';
import { createNewSession, ensureSession } from '../../stores/session-actions';
import { snapshotStreamBuffer } from '../../stores/stream-invalidator';

const mockFetch = vi.mocked(lingxiFetch);
const mockLoadDeskFiles = vi.mocked(loadDeskFiles);
const mockSnapshot = vi.mocked(snapshotStreamBuffer);

function jsonResponse(body: unknown, ok = true): Response {
  return { ok, json: async () => body } as unknown as Response;
}

const OLD_SESSION_PATH = '/session/old-main.jsonl';
const NEW_SESSION_PATH = '/session/new-v033.jsonl';

describe('v0.1.33 新建会话首屏空白（regression: FIXED）', () => {
  beforeEach(() => {
    Object.keys(mockState).forEach(k => delete mockState[k]);
    Object.assign(mockState, initialStateFactory());
    Object.assign(mockState, { workspaceDeskStateByRoot: {} as Record<string, unknown> });
    (globalThis.window as unknown as { hana?: unknown }).hana = {};
    installStoreMethods();
    mockFetch.mockReset();
    mockLoadDeskFiles.mockReset();
    streamResumeMocks.requestStreamResume.mockReset();
    websocketMocks.requestTerminalSnapshotForCurrentSession.mockClear();
    deskActionMocks.activateWorkspaceDesk.mockReset();
    deskActionMocks.activateWorkspaceDesk.mockImplementation(async (root?: string | null, options?: { mountId?: string | null; label?: string | null }) => {
      const mountId = options?.mountId || null;
      const normalized = mountId ? `studio:${mountId}` : (root || '');
      const currentRoot = (mockState.deskWorkspaceMountId as string | null)
        ? `studio:${mockState.deskWorkspaceMountId as string}`
        : ((mockState.deskBasePath as string) || '');
      const states = mockState.workspaceDeskStateByRoot as Record<string, any>;
      if (currentRoot) {
        states[currentRoot] = {
          deskCurrentPath: (mockState.deskCurrentPath as string) || '',
          deskFiles: mockState.deskFiles,
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
        mockState.deskJianContent = null;
        return;
      }
      const saved = states[normalized] || null;
      const nextSubdir = currentRoot === normalized
        ? ((mockState.deskCurrentPath as string) || '')
        : (saved?.deskCurrentPath || '');
      mockState.deskBasePath = normalized;
      mockState.deskWorkspaceMountId = mountId;
      mockState.deskWorkspaceLabel = options?.label || null;
      mockState.deskCurrentPath = nextSubdir;
      mockState.deskFiles = [];
      mockState.deskJianContent = null;
      deskActionMocks.loadDeskFiles(nextSubdir, mountId ? null : normalized, mountId);
    });
    mockSnapshot.mockReset();
    mockSnapshot.mockReturnValue(null);
    clearMessageLiveVersion();
    dispatchedEvents.length = 0;
  });

  it('regression: FIXED — 新会话首条消息后历史被真实加载：不种空缓存，switchSession 走 loadMessages，无 WS 事件也有内容', async () => {
    // ── 场景：用户正停留在旧会话（主工作台），点「新建聊天」并发送首条消息 ──
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
        cwd: '/workspace/old-main',
      }],
      deskBasePath: '/workspace/old-main',
      deskCurrentPath: 'notes',
      deskFiles: [{ name: 'old-note.md' }],
      welcomeVisible: false,
    });

    // ── createNewSession：进入 pending 新会话草稿视图 ──
    mockFetch.mockImplementation(async (url: string) => {
      if (url === '/api/preferences/session-permission-default') {
        return jsonResponse({ permissionMode: 'ask' });
      }
      if (url === '/api/session-thinking-level?pendingNewSession=1') {
        return jsonResponse({ thinkingLevel: 'medium' });
      }
      throw new Error(`unexpected fetch during createNewSession: ${url}`);
    });

    await createNewSession();

    expect(mockState.pendingNewSession).toBe(true);
    expect(mockState.currentSessionPath).toBeNull();
    expect(mockState.welcomeVisible).toBe(true);

    // ── 发送首条消息 → ensureSession：new-detached + switch 全部成功返回 ──
    mockFetch.mockReset();
    mockFetch.mockImplementation(async (url: string) => {
      if (url === '/api/sessions/new-detached') {
        return jsonResponse({
          ok: true,
          path: NEW_SESSION_PATH,
          sessionId: 'sess_new_v033',
          agentId: 'hana',
          cwd: '/workspace/old-main',
          workspaceFolders: [],
        });
      }
      if (url === '/api/sessions/switch') {
        return jsonResponse({
          ok: true,
          path: NEW_SESSION_PATH,
          sessionId: 'sess_new_v033',
          agentId: 'hana',
          cwd: '/workspace/old-main',
          workspaceFolders: [],
          memoryEnabled: true,
          permissionMode: 'ask',
        });
      }
      // /api/sessions/messages 理论上可达——本测试断言它绝不会被调用（见下）。
      if (String(url).startsWith('/api/sessions/messages')) {
        return jsonResponse({ messages: [{ role: 'user', content: '第一条消息' }], blocks: [], todos: [], hasMore: false });
      }
      throw new Error(`unexpected fetch during ensureSession: ${url}`);
    });

    const ref = await ensureSession();

    expect(ref).toMatchObject({ sessionId: 'sess_new_v033', sessionPath: NEW_SESSION_PATH });

    // ── 修复后三事实：首屏由历史加载兜底 ──
    // 事实 1：欢迎页已经消失（switchSession 批量 setState 里 welcomeVisible: false）
    expect(mockState.welcomeVisible).toBe(false);
    // 事实 2：currentSessionPath 已指向新会话（视图认定「已在会话里」）
    expect(mockState.currentSessionPath).toBe(NEW_SESSION_PATH);
    expect(mockState.pendingNewSession).toBe(false);
    // 事实 3：该会话消息缓存已从 /api/sessions/messages hydrate——不再依赖 WS 事件
    expect(mockFetch.mock.calls.some(([url]) => String(url).startsWith('/api/sessions/messages'))).toBe(true);
    const cache = (mockState.chatSessions as Record<string, { items: unknown[] } | undefined>)[NEW_SESSION_PATH];
    expect(cache).toBeTruthy();
    expect(cache?.items).toHaveLength(1);
  });
});
