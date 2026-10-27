/** 会话异步操作绑定真实连接和变更次数；离开后返回也不能接收旧结果。 */
import { useStore } from '../stores';
import { SIDE_CHAT_INITIAL_STATE } from '../stores/side-chat-slice';
import { resetSessionMemoryCache } from '../components/input/composer-memory-mode';
import { appendConnectionAuth, buildConnectionUrl, resolveServerConnection, type ServerConnection } from './server-connection';

export interface SessionConnectionOwner { key: string; revision: number; connection: ServerConnection }
let installed = false;
let key = '';
let revision = 0;
let previousRust = false;
let selectedSession: string | null = null;
let selectionRevision = 0;
const listeners = new Set<() => void>();
function identity(): string {
  const connection = resolveServerConnection(useStore.getState());
  return connection ? JSON.stringify(connection, (_name, value: unknown) => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return value;
    return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b)));
  }) : '';
}
function observe(): void {
  if (installed) return;
  installed = true; key = identity(); selectedSession = useStore.getState().currentSessionId; previousRust = resolveServerConnection(useStore.getState())?.serverNodeKind === 'lingxi-service';
  useStore.subscribe(() => {
    const next = identity();
    const target = useStore.getState().currentSessionId;
    const selectionChanged = target !== selectedSession;
    if (selectionChanged) { selectedSession = target; selectionRevision++; }
    if (next === key) { if (selectionChanged) for (const listener of listeners) listener(); return; }
    key = next; revision++;
    const nextRust = resolveServerConnection(useStore.getState())?.serverNodeKind === 'lingxi-service';
    const reset = previousRust || nextRust; previousRust = nextRust;
    if (reset) {
      resetSessionMemoryCache();
      useStore.setState({ sessions: [], chatSessions: {}, sessionLocatorsById: {},
      currentSessionId: null, currentSessionPath: null, currentSessionPathOverride: null, pendingSessionSwitchPath: null,
      sideChat: { ...SIDE_CHAT_INITIAL_STATE, createdAt: Date.now() },
      mapActiveWorkspaceKey: null, mapTurnsBySessionId: {}, mapCameraByWorkspace: {},
      mapSelectedCardId: null, mapCardPositions: {}, mapCollapsedCardIds: [], mapLayoutLoaded: false,
      mapDraft: null, mapPendingTurns: {},
      drafts: {}, draftDocs: {}, draftsHydratedAt: 0, pendingNewSession: false, pendingDraftId: null,
      pendingProjectId: null, pendingNewSessionThinkingLevel: null, pendingNewSessionPermissionMode: null,
      models: [], currentModel: null, sessionModelsByPath: {}, thinkingLevelBySession: {}, thinkingLevel: 'medium', currentAgentId: null,
      selectedFolder: null, selectedWorkspaceMountId: null, selectedAgentId: null, workspaceFolders: [],
      deskFiles: [], deskTreeFilesByPath: {}, deskBasePath: '', deskWorkspaceMountId: null,
      agentActivitiesBySession: {} });
    }
    for (const listener of listeners) listener();
  });
}
export function sessionConnectionRevision(): number { observe(); return revision; }
export function sessionSelectionRevision(): number { observe(); return selectionRevision; }
export function subscribeSessionConnection(listener: () => void): () => void {
  observe(); listeners.add(listener); return () => listeners.delete(listener);
}
export function captureSessionConnection(): SessionConnectionOwner | null {
  const connection = resolveServerConnection(useStore.getState());
  if (!connection) return null;
  observe();
  return { key: identity(), revision, connection: structuredClone(connection) };
}
export function ownsSessionConnection(owner: SessionConnectionOwner | null): owner is SessionConnectionOwner {
  observe(); return !!owner && owner.key === identity() && owner.revision === revision;
}
/** 认证和地址都来自捕获连接；发出前和返回后均拒绝任何连接变更。 */
export async function sessionOwnerFetch(owner: SessionConnectionOwner, path: string, options: RequestInit = {}): Promise<Response> {
  if (!ownsSessionConnection(owner)) throw new Error('连接已切换，请重新操作');
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 30_000);
  const stop = subscribeSessionConnection(() => { if (!ownsSessionConnection(owner)) controller.abort(); });
  const abort = () => controller.abort();
  if (options.signal?.aborted) abort();
  else options.signal?.addEventListener('abort', abort, { once: true });
  try {
    const response = await fetch(buildConnectionUrl(owner.connection, path), { ...options,
      headers: appendConnectionAuth(owner.connection, options.headers), signal: controller.signal });
    if (!ownsSessionConnection(owner)) throw new Error('连接已切换，已忽略旧结果');
    if (!response.ok) {
      const detail = await response.json().catch(() => null) as { message?: unknown; error?: unknown; code?: unknown } | null;
      if (!ownsSessionConnection(owner)) throw new Error('连接已切换，已忽略旧错误');
      const message = typeof detail?.message === 'string' ? detail.message : typeof detail?.error === 'string' ? detail.error : `请求失败（${response.status}）`;
      throw Object.assign(new Error(message), typeof detail?.code === 'string' ? { code: detail.code } : {});
    }
    return response;
  } finally { clearTimeout(timeout); stop(); options.signal?.removeEventListener('abort', abort); }
}
