import { useStore } from '../stores';
import { lingxiFetch } from '../hooks/use-hana-fetch';
import { sessionIdForPathFromLocatorState, sessionScopedValue } from '../stores/session-slice';
import { applyAgentIdentity, loadAvatars } from '../stores/agent-actions';
import { activateWorkspaceDesk } from '../stores/desk-actions';
import { loadMessages, pendingNewSessionIdentityPatch } from '../stores/session-actions';
import { connectWebSocket, getWebSocket } from '../services/websocket';
import { configureAppEventActions } from '../services/app-event-actions';
import { configureWsMessageHandler } from '../services/ws-message-handler';
import { captureSessionConnection, ownsSessionConnection, sessionSelectionRevision } from '../services/session-connection-owner';
import { resolveServerConnection, createBrowserServerConnection, upsertServerConnection, warnIfServerProtocolMismatch, type ServerIdentity } from '../services/server-connection';
import { loadModels } from '../utils/ui-helpers';
import { applySyncedAppearancePreferences, type SyncedAppearancePreferences } from '../services/appearance-sync';
import { applyChatLayout } from '../chat/layout';
import { applyEditorTypography } from '../editor/typography';
import type { ThinkingLevel } from '../stores/model-slice';
import type { Agent, Model, Session, SessionPermissionMode } from '../types';
import type { ClientAgentSummary, ClientModelSummary, MobileBootstrapResponse } from '../../../../contracts/generated/ts/lingxi-protocol';

export interface MobilePrincipal {
  kind?: string | null;
  credentialKind?: string | null;
  connectionKind?: string | null;
  trustState?: string | null;
  serverId?: string | null;
  serverNodeId?: string | null;
  userId?: string | null;
  studioId?: string | null;
  deviceId?: string | null;
  credentialId?: string | null;
  platformAccountId?: string | null;
  officialServiceKind?: string | null;
  scopes?: string[];
}

export interface MobileAuthSession {
  authenticated: boolean;
  principal: MobilePrincipal | null;
  backend: 'node' | 'rust';
}

export interface MobileBootstrap {
  locale?: string;
  agentName?: string;
  userName?: string;
  currentAgentId?: string | null;
  agentYuan?: string;
  homeFolder?: string | null;
  cwdHistory?: string[];
  memoryMasterEnabled?: boolean;
  memoryEnabled?: boolean;
  thinkingLevel?: ThinkingLevel;
  chat?: unknown;
  editor?: unknown;
  avatars?: Record<string, boolean>;
  agents?: Agent[];
  appearance?: SyncedAppearancePreferences;
  permissionMode?: SessionPermissionMode;
  models?: Model[];
  activeModel?: { id: string; provider: string } | null;
  currentModel?: { id: string; provider: string } | null;
  modelState?: string;
  modelGeneration?: string;
}

let mobileHandlersConfigured = false;
let runtimeRevision = 0;

/** 离开登录周期后，尚未返回的初始化不得再更新页面，并断开旧主体可能仍在用的 WS。 */
export function invalidateMobileRuntime(): void {
  runtimeRevision++;
  // 代次只挡页面写入；旧主体（已撤销/已过期）的 WS 必须真实关闭，
  // 否则退出登录后旧连接仍会继续接收并显示事件。
  getWebSocket()?.close();
}

/** Rust 助手摘要 → 渲染层 Agent 形状（字段一一对应，不增补不丢字段）。 */
function projectRustAgentSummary(summary: ClientAgentSummary): Agent {
  return {
    id: summary.id,
    name: summary.name,
    yuan: summary.yuan,
    isPrimary: summary.isPrimary,
    hasAvatar: summary.hasAvatar,
    homeFolder: summary.homeFolder ?? null,
    effectiveHomeFolder: summary.effectiveHomeFolder ?? null,
    memoryMasterEnabled: summary.memoryMasterEnabled,
  };
}

const THINKING_LEVELS: readonly string[] = ['off', 'auto', 'low', 'medium', 'high', 'xhigh', 'max'];
function asThinkingLevel(value: unknown): ThinkingLevel | undefined {
  return typeof value === 'string' && THINKING_LEVELS.includes(value) ? value as ThinkingLevel : undefined;
}
const MODEL_INPUTS = ['text', 'image', 'video'] as const;
type ModelInput = (typeof MODEL_INPUTS)[number];
function asModelInput(value: unknown): ModelInput | undefined {
  return typeof value === 'string' && (MODEL_INPUTS as readonly string[]).includes(value) ? value as ModelInput : undefined;
}

/** Rust 模型摘要 → 渲染层 Model：thinkingLevels/defaultThinkingLevel 按已知档位
    白名单过滤（协议侧是 string，非法档位不得落进 UI 的档位选择）。 */
function projectRustModelSummary(summary: ClientModelSummary): Model {
  return {
    ...summary,
    thinkingLevels: (summary.thinkingLevels || [])
      .map(asThinkingLevel)
      .filter((level): level is ThinkingLevel => !!level),
    defaultThinkingLevel: asThinkingLevel(summary.defaultThinkingLevel),
    input: (summary.input || [])
      .map(asModelInput)
      .filter((value): value is ModelInput => !!value),
  };
}

/** Rust 正式 bootstrap 投影为页面消费的引导形状：agents 投影为 Agent[]，
    permissionMode / thinkingLevel 白名单过滤；其余字段已是正式响应的一部分，原样透传。 */
function projectRustBootstrap(raw: MobileBootstrapResponse): MobileBootstrap {
  const { agents, permissionMode, thinkingLevel, appearance, editor, chat, models, ...rest } = raw;
  return {
    ...rest,
    appearance: (appearance ?? undefined) as SyncedAppearancePreferences | undefined,
    editor,
    chat,
    thinkingLevel: asThinkingLevel(thinkingLevel),
    agents: agents.map(projectRustAgentSummary),
    models: (models || []).map(projectRustModelSummary),
    permissionMode: isSessionPermissionMode(permissionMode) ? permissionMode : undefined,
  };
}

function captureMobileTarget(): () => void {
  const owner = captureSessionConnection();
  const revision = sessionSelectionRevision();
  const state = useStore.getState();
  const target = JSON.stringify([state.currentSessionPath, state.currentAgentId, state.pendingNewSession, state.selectedAgentId]);
  return () => {
    const current = useStore.getState();
    if ((owner && !ownsSessionConnection(owner)) || sessionSelectionRevision() !== revision
      || JSON.stringify([current.currentSessionPath, current.currentAgentId, current.pendingNewSession, current.selectedAgentId]) !== target) {
      throw new Error('手机聊天目标已切换，已忽略旧结果');
    }
  };
}

/** 普通聊天权限下的会话摘要；不制造磁盘修订点或本机路径。 */
export function projectMobileRustSessions(value: unknown, agents: Agent[]): Session[] {
  // 正式 GET /lingxi/v1/sessions 的真实形状是 { sessions: [...] } 包装；
  // 裸数组不是该路由的输出，收到即不完整。
  const rows = (value as { sessions?: unknown } | null)?.sessions;
  if (!Array.isArray(rows)) throw new Error('mobile sessions response is incomplete');
  return rows.map(row => {
    if (!row || typeof row.sessionId !== 'string' || !row.sessionId.trim()
      || typeof row.agentId !== 'string' || typeof row.ownerUserId !== 'string' || typeof row.title !== 'string'
      || !Number.isSafeInteger(row.runCount) || row.runCount < 0 || !Array.isArray(row.lastRuns)) {
      throw new Error('mobile sessions response is incomplete');
    }
    const times = row.lastRuns.map((run: { startedAtUnixMs?: unknown }) => run?.startedAtUnixMs);
    if (times.some((time: unknown) => typeof time !== 'number' || !Number.isSafeInteger(time) || time < 0 || time > 8.64e15)) {
      throw new Error('mobile sessions response is incomplete');
    }
    return { path: `native:${row.sessionId}`, sessionId: row.sessionId, agentId: row.agentId,
      agentName: agents.find(agent => agent.id === row.agentId)?.name || row.agentId,
      title: row.title, firstMessage: '', modified: times.length ? new Date(Math.max(...times)).toISOString() : '',
      messageCount: 0, cwd: null, revision: null };
  });
}

export async function readMobileAuthSession(): Promise<MobileAuthSession> {
  // 旧服务的会话入口优先；它明确拒绝或不存在时，才核对 Rust 的公开身份。
  const legacy = await fetch('/api/web-auth/session', {
    headers: { Accept: 'application/json' },
    credentials: 'same-origin',
  });
  if (legacy.ok) {
    const session = await mobileJson<Omit<MobileAuthSession, 'backend'>>(legacy, '/api/web-auth/session');
    return { ...requireMobileSession(session), backend: 'node' };
  }
  if (![401, 403, 404].includes(legacy.status)) {
    throw new Error(`mobile session request failed: ${legacy.status} ${legacy.statusText}`);
  }
  const health = await fetch('/lingxi/v1/health', {
    headers: { Accept: 'application/json' },
    credentials: 'same-origin',
  });
  if (!health.ok) {
    throw new Error(`mobile server identity could not be verified: ${health.status} ${health.statusText}`);
  }
  const identity = await mobileJson<Record<string, unknown>>(health, '/lingxi/v1/health');
  if (identity.serverKind !== 'lingxi-service' || identity.status !== 'ok') {
    throw new Error('mobile server identity could not be verified');
  }
  const session = await rawJson<Omit<MobileAuthSession, 'backend'>>('/lingxi/v1/web-auth/session');
  return { ...requireMobileSession(session), backend: 'rust' };
}

function requireMobileSession(value: Omit<MobileAuthSession, 'backend'>): Omit<MobileAuthSession, 'backend'> {
  if (!value || typeof value.authenticated !== 'boolean'
    || (value.authenticated && (!value.principal || typeof value.principal !== 'object'))) {
    throw new Error('mobile session response is incomplete');
  }
  return value;
}

async function mobileJson<T>(response: Response, path: string): Promise<T> {
  const contentType = response.headers.get('Content-Type');
  if (contentType && !/\bapplication\/(?:[\w.-]+\+)?json\b/i.test(contentType)) {
    throw new Error(`mobile server returned non-JSON response for ${path}`);
  }
  try {
    return await response.json() as T;
  } catch {
    throw new Error(`mobile server returned invalid JSON for ${path}`);
  }
}

export async function initializeMobileRuntime(principal: MobilePrincipal, backend: MobileAuthSession['backend'], options: { isCurrent?: () => boolean } = {}): Promise<{
  identity: ServerIdentity;
  bootstrap: MobileBootstrap;
}> {
  const revision = ++runtimeRevision;
  const initialConnection = JSON.stringify(resolveServerConnection(useStore.getState()));
  const assertRuntime = () => {
    if (revision !== runtimeRevision || options.isCurrent?.() === false) throw new Error('手机登录已切换，已忽略旧结果');
  };
  configureMobileMessageHandlers();

  const identity = await rawJson<ServerIdentity>(backend === 'rust'
    ? '/lingxi/v1/server/identity'
    : '/api/server/identity');
  if (backend === 'rust') {
    if (identity.serverNodeKind !== 'lingxi-service') {
      throw new Error('Rust mobile server identity does not match');
    }

  }
  assertRuntime();
  if (JSON.stringify(resolveServerConnection(useStore.getState())) !== initialConnection) throw new Error('连接已切换，已忽略旧登录结果');
  warnIfServerProtocolMismatch(identity);
  const connection = createBrowserServerConnection({
    identity,
    principal,
    origin: window.location.origin,
  });

  useStore.setState({
    serverConnections: upsertServerConnection(useStore.getState().serverConnections, connection),
    activeServerConnectionId: connection.connectionId,
    activeServerConnection: connection,
    serverPort: window.location.port || null,
    serverToken: null,
    currentTab: 'chat',
    sidebarOpen: false,
    jianOpen: false,
    previewOpen: false,
    currentSessionPath: null,
    pendingSessionSwitchPath: null,
    // 换主体/换连接时旧模型状态与代次不得残留（避免旧 ready 覆盖新 unconfigured）。
    modelState: null,
    modelGeneration: null,
    ...pendingNewSessionIdentityPatch(),
    welcomeVisible: true,
  });

  const owner = captureSessionConnection();
  const assertInitialTarget = captureMobileTarget();
  const assertOwner = () => {
    assertRuntime();
    if (!ownsSessionConnection(owner)) throw new Error('手机连接已切换，已忽略旧初始化');
  };
  const native = backend === 'rust';
  const bootstrapRes = await lingxiFetch(native ? '/lingxi/v1/mobile/bootstrap' : '/api/mobile/bootstrap');
  const bootstrap = native
    ? projectRustBootstrap(await bootstrapRes.json() as MobileBootstrapResponse)
    : await bootstrapRes.json() as MobileBootstrap;
  assertOwner();
  assertInitialTarget();
  if (!bootstrap || typeof bootstrap !== 'object' || !Array.isArray(bootstrap.agents)) throw new Error('mobile bootstrap response is incomplete');
  // Rust bootstrap 自带权限默认值（手机不能访问 settings 权限面）；旧服务另读该面。
  const permissionDefault = native
    ? { permissionMode: bootstrap.permissionMode }
    : await rawJson<{ permissionMode?: SessionPermissionMode }>('/api/preferences/session-permission-default');
  assertOwner();
  applySyncedAppearancePreferences(bootstrap.appearance);
  applyEditorTypography(bootstrap.editor);
  applyChatLayout(bootstrap.chat);

  if (window.i18n?.load) {
    await window.i18n.load(bootstrap.locale || 'zh-CN');
    assertOwner();
    assertInitialTarget();
    useStore.setState({ locale: window.i18n.locale });
  }

  await applyAgentIdentity({
    agentName: bootstrap.agentName || 'Lingxi',
    agentId: bootstrap.currentAgentId || undefined,
    userName: bootstrap.userName || window.t?.('common.user') || 'User',
    yuan: bootstrap.agentYuan,
    ui: { avatars: false, agents: false, welcome: true },
  });
  assertOwner();
  useStore.setState({
    agents: Array.isArray(bootstrap.agents) ? bootstrap.agents : [],
  });
  const currentAgent = (bootstrap.agents || []).find((agent) => agent.id === bootstrap.currentAgentId)
    || (native ? undefined : (bootstrap.agents || []).find((agent) => agent.isPrimary) || (bootstrap.agents || [])[0]);
  if (currentAgent) {
    const homeFolder = bootstrap.homeFolder || currentAgent.homeFolder || null;
    useStore.setState({
      currentAgentId: currentAgent.id,
      agentName: currentAgent.name,
      agentYuan: currentAgent.yuan || bootstrap.agentYuan || 'lingxi',
      homeFolder,
      selectedFolder: homeFolder,
      cwdHistory: Array.isArray(bootstrap.cwdHistory) ? bootstrap.cwdHistory : [],
      memoryMasterEnabled: typeof bootstrap.memoryMasterEnabled === 'boolean'
        ? bootstrap.memoryMasterEnabled
        : currentAgent.memoryMasterEnabled !== false,
      memoryEnabled: typeof bootstrap.memoryEnabled === 'boolean' ? bootstrap.memoryEnabled : useStore.getState().memoryEnabled,
      ...(bootstrap.thinkingLevel ? { thinkingLevel: bootstrap.thinkingLevel } : {}),
    });
  }
  loadAvatars(bootstrap.avatars, currentAgent?.id || bootstrap.currentAgentId || null);
  if (isSessionPermissionMode(permissionDefault.permissionMode)) {
    useStore.getState().setPendingNewSessionPermissionMode(permissionDefault.permissionMode);
  }

  // 会话列表先定局：空列表会落地新会话草稿身份；若与模型加载并行，
  // 草稿身份翻转会被聊天归属保护如实判为串线（真实时序竞争，非夹具问题）。
  await loadMobileSessions({ selectFirst: false });
  assertOwner();
  await Promise.all([
    loadModels({ throwOnError: true }),
    activateMobileWelcomeDesk(),
  ]);

  // Rust bootstrap 已带模型面真实状态与代次：存只读镜像，不把空目录当 ready。
  if (native) {
    useStore.setState({
      modelState: typeof bootstrap.modelState === 'string' ? bootstrap.modelState : null,
      modelGeneration: typeof bootstrap.modelGeneration === 'string' ? bootstrap.modelGeneration : null,
    });
  }

  assertOwner();
  connectWebSocket();

  return { identity, bootstrap };
}

export async function loadMobileSessions({
  selectFirst = false,
}: {
  selectFirst?: boolean;
} = {}): Promise<Session[]> {
  const assertTarget = captureMobileTarget();
  const stateBefore = useStore.getState();
  const native = resolveServerConnection(stateBefore)?.serverNodeKind === 'lingxi-service';
  const res = await lingxiFetch(native ? '/lingxi/v1/sessions' : '/api/sessions');
  const raw = await res.json() as unknown;
  assertTarget();
  const sessions = native ? projectMobileRustSessions(raw, stateBefore.agents) : raw;
  if (!Array.isArray(sessions)
    || !sessions.every((session) => session && typeof session.path === 'string' && session.path.trim())) {
    throw new Error('mobile sessions response is incomplete');
  }
  const next = sessions as Session[];
  useStore.setState(state => ({ sessions: next, sessionLocatorsById: {
    ...state.sessionLocatorsById,
    ...Object.fromEntries(next.flatMap(session => session.sessionId ? [[session.sessionId, { path: session.path }]] : [])),
  } }));

  const state = useStore.getState();
  const currentStillExists = !!state.currentSessionPath && next.some((session) => session.path === state.currentSessionPath);
  const target = currentStillExists
    ? state.currentSessionPath
    : selectFirst
      ? next[0]?.path || null
      : null;
  const targetSession = target ? next.find((session) => session.path === target) || null : null;

  if (target && target !== state.currentSessionPath) {
    await switchMobileSession(target, targetSession);
  } else if (target && !sessionScopedValue(useStore.getState(), useStore.getState().chatSessions, target)) {
    syncMobilePermissionMode(targetSession);
    await activateMobileSessionDesk(targetSession);
    await loadMessages(target);
  } else if (target) {
    syncMobilePermissionMode(targetSession);
    await activateMobileSessionDesk(targetSession);
    assertTarget();
    if (native) await loadMessages(target);
  } else if (!target) {
    useStore.setState({
      currentSessionPath: null,
      pendingSessionSwitchPath: null,
      ...pendingNewSessionIdentityPatch(),
      welcomeVisible: true,
    });
  }

  return next;
}

export async function switchMobileSession(
  path: string,
  session?: Pick<Session, 'cwd' | 'permissionMode' | 'sessionId' | 'agentId' | 'agentName'> | null,
): Promise<void> {
  useStore.setState((state) => {
    const sessionId = typeof session?.sessionId === 'string' && session.sessionId.trim()
      ? session.sessionId.trim()
      : sessionIdForPathFromLocatorState(state, path);
    const sessionAgent = session?.agentId
      ? state.agents.find(agent => agent.id === session.agentId) || null
      : null;
    return {
    currentSessionPath: path,
    currentSessionId: sessionId,
    ...(sessionId ? {
      sessionLocatorsById: {
        ...state.sessionLocatorsById,
        [sessionId]: { path },
      },
    } : {}),
    pendingSessionSwitchPath: path,
    pendingNewSession: false,
    selectedAgentId: null,
    welcomeVisible: false,
    ...(session?.agentId ? {
      currentAgentId: session.agentId,
      agentName: session.agentName || sessionAgent?.name || session.agentId,
      agentYuan: sessionAgent?.yuan || 'lingxi',
      homeFolder: sessionAgent?.homeFolder || null,
    } : {}),
    };
  });
  const assertTarget = captureMobileTarget();
  syncMobilePermissionMode(session || null);
  try {
    await activateMobileSessionDesk(session || null);
    assertTarget();
    await loadMessages(path);
    assertTarget();
    await loadModels();
  } finally {
    useStore.setState((state) => (
      state.pendingSessionSwitchPath === path && state.currentSessionPath === path ? { pendingSessionSwitchPath: null } : {}
    ));
  }
  requestContextUsage(path);
}

export async function createMobileSession(): Promise<string | null> {
  const state = useStore.getState();
  const assertTarget = captureMobileTarget();
  const native = resolveServerConnection(state)?.serverNodeKind === 'lingxi-service';
  const body: Record<string, unknown> = {
    memoryEnabled: state.memoryEnabled,
    currentSessionPath: state.currentSessionPath,
  };
  if (state.selectedFolder) body.cwd = state.selectedFolder;
  if (state.workspaceFolders?.length) body.workspaceFolders = state.workspaceFolders;
  if (state.pendingNewSessionThinkingLevel) body.thinkingLevel = state.pendingNewSessionThinkingLevel;
  if (state.selectedAgentId && state.selectedAgentId !== state.currentAgentId) {
    body.agentId = state.selectedAgentId;
  }

  if (native) body.agentId = state.selectedAgentId || state.currentAgentId;
  const res = await lingxiFetch(native ? '/lingxi/v1/sessions/new' : '/api/sessions/new', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  const data = await res.json() as {
    path?: string;
    sessionId?: string | null;
    cwd?: string | null;
    workspaceFolders?: string[];
    agentId?: string | null;
    agentName?: string | null;
    permissionMode?: SessionPermissionMode | null;
  };
  assertTarget();
  if (native && (!data.sessionId || data.path !== `native:${data.sessionId}`)) throw new Error('手机会话创建身份无效');
  if (!data.path) return null;
  const patch: Record<string, unknown> = {
    currentSessionPath: data.path,
    currentSessionId: typeof data.sessionId === 'string' && data.sessionId.trim() ? data.sessionId.trim() : null,
    pendingSessionSwitchPath: null,
    pendingNewSession: false,
    welcomeVisible: false,
    pendingNewSessionThinkingLevel: null,
    workspaceFolders: Array.isArray(data.workspaceFolders) ? data.workspaceFolders : [],
    selectedAgentId: null,
  };
  if (patch.currentSessionId) {
    patch.sessionLocatorsById = {
      ...useStore.getState().sessionLocatorsById,
      [patch.currentSessionId as string]: { path: data.path },
    };
  }
  if (data.agentId) {
    patch.currentAgentId = data.agentId;
    if (data.agentName) patch.agentName = data.agentName;
  }
  useStore.setState(patch);
  syncMobilePermissionMode(data);
  useStore.getState().initSession(data.path, [], false);
  await activateMobileSessionDesk({ cwd: data.cwd || null });
  await loadMobileSessions();
  loadModels();
  return data.path;
}

async function activateMobileSessionDesk(session: Pick<Session, 'cwd'> | null | undefined): Promise<void> {
  await activateWorkspaceDesk(session?.cwd || null);
}

async function activateMobileWelcomeDesk(): Promise<void> {
  const state = useStore.getState();
  await activateWorkspaceDesk(state.selectedFolder || state.homeFolder || null);
  useStore.setState({ previewOpen: false });
}

function syncMobilePermissionMode(session: Pick<Session, 'permissionMode'> | null | undefined): void {
  const mode = session?.permissionMode;
  if (!isSessionPermissionMode(mode)) return;
  window.dispatchEvent(new CustomEvent('hana-plan-mode', {
    detail: {
      enabled: mode === 'read_only',
      mode,
    },
  }));
}

function isSessionPermissionMode(value: unknown): value is SessionPermissionMode {
  return value === 'auto' || value === 'operate' || value === 'ask' || value === 'read_only';
}

function configureMobileMessageHandlers(): void {
  if (mobileHandlersConfigured) return;
  mobileHandlersConfigured = true;
  configureAppEventActions({ requestContextUsage });
  configureWsMessageHandler({ requestContextUsage });
}

function requestContextUsage(sessionPath: string): void {
  const ws = getWebSocket();
  if (ws?.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: 'context_usage', sessionPath }));
  }
}

async function rawJson<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers);
  headers.set('Accept', 'application/json');
  if (options.body && !headers.has('Content-Type')) headers.set('Content-Type', 'application/json');
  const res = await fetch(path, {
    ...options,
    headers,
    credentials: 'same-origin',
  });
  if (!res.ok) {
    let detail = `${res.status} ${res.statusText}`;
    try {
      const data = await res.json();
      detail = data.detail || data.error || detail;
    } catch {}
    throw new Error(detail);
  }
  return await mobileJson<T>(res, path);
}
