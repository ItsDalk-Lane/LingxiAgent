/**
 * session-actions.ts — Session 生命周期操作（纯逻辑 + API）
 *
 * 从 sidebar-shim.ts 迁移。所有函数直接操作 Zustand store，
 * 不依赖 ctx 注入，不持有闭包状态（除 _switchVersion 防竞争）。
 */

/* eslint-disable @typescript-eslint/no-explicit-any -- store partial patch + API 响应 JSON */

import { useStore } from './index';
import { appendConnectionAuth, buildConnectionUrl, type ServerConnection } from '../services/server-connection';
import { sessionScopedKey, sessionScopedListIncludes, sessionScopedValue } from './session-slice';
import { lingxiFetch, lingxiUrl } from '../hooks/use-hana-fetch';
import {
  conditionalMessagesFetch,
  hasMessagesValidationRecord,
  headerGet as responseHeaderGet,
  negotiatedHistoryPageLimit,
  saveHistoryValidationRecord,
} from './history-protocol-client';
import { hydrateInputDrafts } from './input-draft-persistence';
import { HOME_DRAFT_KEY } from '../../../../shared/input-drafts.ts';
import { normalizeWorkspacePath } from '../../../../shared/workspace-history.ts';
import { buildItemsFromHistory } from '../utils/history-builder';
import { migrateLegacyTodos } from '../utils/todo-compat';
import { clearChat as clearChatAction } from './agent-actions';
import { activateWorkspaceDesk } from './desk-actions';
import { loadModels } from '../utils/ui-helpers';
import { browserStateForPath, setBrowserStateForPath } from './browser-slice';
import { computerOverlayForSession } from './computer-overlay-slice';
import { snapshotStreamBuffer, type StreamBufferSnapshot } from './stream-invalidator';
import { errorWithCode, presentError, presentErrorWithLabel } from '../errors/error-presenter';
import { normalizeSessionRouteError } from '../../../../shared/error-user-messages.ts';
import type { ChatMessage, ContentBlock } from './chat-types';
import { readMessageLiveVersion } from './message-live-version';
import type { SessionMetaRecoveryStatus, SessionPermissionMode, TodoItem } from '../types';
import { findPrimaryAgent, resolveAgentWorkspace } from '../utils/agent-workspace';

// ── 防竞争计数器 ──

let _switchVersion = 0;
let _switchAbortController: AbortController | null = null;
let _pendingDraftSequence = 0;

export interface SessionRef {
  sessionId: string;
  sessionPath: string;
  agentId: string;
}

function nextPendingDraftId(): string {
  _pendingDraftSequence += 1;
  return `pending-${Date.now().toString(36)}-${_pendingDraftSequence.toString(36)}`;
}

/**
 * pending 新会话身份补丁：任何把 store 切入 "welcome / 待建会话" 态的入口都必须
 * 展开这个补丁，而不是裸设 `pendingNewSession: true`。currentPendingSessionDraft()
 * 要求 pendingDraftId 非空才认这是一个有效的待建草稿；裸设会让身份残缺，
 * submitEditorMessage 的 ensureSession 门槛就此失效，发送静默无响应（#2101）。
 */
export function pendingNewSessionIdentityPatch(): { pendingNewSession: true; pendingDraftId: string } {
  return { pendingNewSession: true, pendingDraftId: nextPendingDraftId() };
}

function invalidateSessionSwitches(): void {
  _switchVersion += 1;
  _switchAbortController?.abort();
  _switchAbortController = null;
  useStore.setState({ pendingSessionSwitchPath: null });
}

function isCurrentSwitch(version: number, path: string): boolean {
  const state = useStore.getState();
  return version === _switchVersion && state.pendingSessionSwitchPath === path;
}

function normalizeSessionId(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null;
}

function mergeSessionLocators(current: Record<string, { path: string | null }> = {}, sessions: any[] = []) {
  const next = { ...current };
  for (const session of Array.isArray(sessions) ? sessions : []) {
    const sessionId = normalizeSessionId(session?.sessionId);
    if (!sessionId) continue;
    next[sessionId] = { path: typeof session.path === 'string' ? session.path : null };
  }
  return next;
}

function sessionIdForPathFromState(state: Record<string, any>, path: string | null): string | null {
  if (!path) return null;
  const session = (state.sessions || []).find((item: any) => item?.path === path);
  return normalizeSessionId(session?.sessionId);
}

function frozenSessionRefFromState(state: Record<string, any>): Readonly<SessionRef> | null {
  const sessionPath = typeof state.currentSessionPath === 'string' && state.currentSessionPath.trim()
    ? state.currentSessionPath
    : null;
  if (!sessionPath) return null;
  const projection = sessionByIdentityOrPath(
    state,
    normalizeSessionId(state.currentSessionId),
    sessionPath,
  );
  const sessionId = normalizeSessionId(state.currentSessionId)
    || normalizeSessionId(projection?.sessionId)
    || sessionIdForPathFromState(state, sessionPath);
  const agentId = normalizeSessionId(projection?.agentId) || normalizeSessionId(state.currentAgentId);
  if (!sessionId || !agentId) return null;
  return Object.freeze({ sessionId, sessionPath, agentId });
}

function frozenSessionRefFromCreateResponse(data: any): Readonly<SessionRef> | null {
  const sessionId = normalizeSessionId(data?.sessionId);
  const sessionPath = typeof data?.path === 'string' && data.path.trim() ? data.path : null;
  const agentId = normalizeSessionId(data?.agentId);
  if (!sessionId || !sessionPath || !agentId) return null;
  return Object.freeze({ sessionId, sessionPath, agentId });
}

function sessionByIdentityOrPath(state: Record<string, any>, sessionId: string | null, sessionPath: string | null): any | null {
  const sessions = Array.isArray(state.sessions) ? state.sessions : [];
  if (sessionId) {
    const byId = sessions.find((item: any) => normalizeSessionId(item?.sessionId) === sessionId);
    if (byId) return byId;
  }
  if (sessionPath) {
    return sessions.find((item: any) => item?.path === sessionPath) || null;
  }
  return null;
}

function currentSessionIdentityPatch(state: Record<string, any>, path: string | null, sessionId: unknown) {
  const normalizedSessionId = normalizeSessionId(sessionId) || sessionIdForPathFromState(state, path);
  return {
    currentSessionPath: path,
    currentSessionId: normalizedSessionId,
    ...(normalizedSessionId ? {
      sessionLocatorsById: {
        ...(state.sessionLocatorsById || {}),
        [normalizedSessionId]: { path },
      },
    } : {}),
  };
}

export interface SessionHistoryEvidencePage {
  messages: Array<{id: string; role: string; clientMessageId?: string; sourceEntryId?: string; snapshotVersion?: number}>;
  hasMore: boolean;
  oldestId?: string;
  reconciliation?: {sessionId: string; sessionPath: string; complete: boolean; snapshotId: string;
    runRevision: number; runStatus: 'running' | 'reconciled_idle' | 'unknown'};
}
/** 只读对账：认证/URL取自捕获连接，不水合编辑器、文件列表或当前会话。 */
export async function fetchSessionHistoryPage(
  connection: ServerConnection,
  sessionRef: {sessionId: string; sessionPath: string},
  options: {before?: string; signal: AbortSignal},
): Promise<SessionHistoryEvidencePage> {
  const params = new URLSearchParams({sessionId:sessionRef.sessionId,path:sessionRef.sessionPath,reconciliation:'1'});
  if (options.before) params.set('before', options.before);
  const response = await fetch(buildConnectionUrl(connection, `/api/sessions/messages?${params}`), {
    headers:appendConnectionAuth(connection),signal:options.signal,
  });
  if (!response.ok) throw new Error(`history_http_${response.status}`);
  const maxBytes = 2 * 1024 * 1024;
  if (Number(response.headers.get('content-length')) > maxBytes) throw new Error('history_response_too_large');
  const reader = response.body?.getReader();
  let text = '';
  if (reader) {
    const decoder = new TextDecoder(); let bytes = 0;
    try {
      while (true) {
        const chunk = await reader.read(); if (chunk.done) break;
        bytes += chunk.value.byteLength;
        if (bytes > maxBytes) throw new Error('history_response_too_large');
        text += decoder.decode(chunk.value, {stream:true});
      }
      text += decoder.decode();
    } finally { await reader.cancel(); reader.releaseLock(); }
  } else {
    text = await response.text();
    if (new TextEncoder().encode(text).byteLength > maxBytes) throw new Error('history_response_too_large');
  }
  const page = JSON.parse(text);
  if (!Array.isArray(page.messages) || typeof page.hasMore !== 'boolean') throw new Error('history_response_invalid');
  return page;
}

function sessionMessagesUrl(path: string, extra: Record<string, string> = {}): string {
  const state = useStore.getState() as Record<string, any>;
  const params = new URLSearchParams();
  params.set('path', path);
  const sessionId = sessionIdForPathFromState(state, path);
  if (sessionId) params.set('sessionId', sessionId);
  for (const [key, value] of Object.entries(extra)) {
    params.set(key, value);
  }
  return `/api/sessions/messages?${params.toString()}`;
}

function putSessionScopedStateValue(
  state: Record<string, any>,
  map: Record<string, any> = {},
  sessionPath: string,
  value: any,
): Record<string, any> {
  const key = sessionScopedKey(state, sessionPath) || sessionPath;
  const next = { ...map, [key]: value };
  if (key !== sessionPath) delete next[sessionPath];
  return next;
}

function deleteSessionScopedStateValue(
  state: Record<string, any>,
  map: Record<string, any> = {},
  sessionPath: string,
): Record<string, any> {
  const key = sessionScopedKey(state, sessionPath) || sessionPath;
  const next = { ...map };
  delete next[key];
  if (key !== sessionPath) delete next[sessionPath];
  return next;
}

function isAbortError(err: unknown): boolean {
  return !!err && typeof err === 'object' && (
    (err as { name?: string }).name === 'AbortError' ||
    (err as { message?: string }).message === 'This operation was aborted'
  );
}

function isDesktopShell(): boolean {
  return typeof window !== 'undefined' && !!(window as unknown as { hana?: unknown }).hana;
}

function shouldRestoreInputFocus(path: string | null): boolean {
  const state = useStore.getState() as Record<string, any>;
  if (!isDesktopShell()) return false;
  if (state.currentTab !== 'chat') return false;
  if (path) {
    if (state.currentSessionPath !== path) return false;
  } else if (state.pendingNewSession !== true || state.currentSessionPath !== null || state.pendingSessionSwitchPath) {
    return false;
  }
  if (state.settingsModal?.open || state.mediaViewer || state.skillViewerData || state.channelCreateOverlayVisible) return false;
  if (path && computerOverlayForSession(state as any, path)) return false;
  return true;
}

function requestChatInputFocus(path: string | null): void {
  if (shouldRestoreInputFocus(path)) useStore.getState().requestInputFocus?.();
}

function isPendingNewSessionDraftView(): boolean {
  const state = useStore.getState() as Record<string, any>;
  return state.pendingNewSession === true
    && state.currentSessionPath === null
    && !state.pendingSessionSwitchPath;
}

const SESSION_PERMISSION_MODES = new Set(['auto', 'operate', 'ask', 'read_only']);

function normalizeSessionPermissionMode(mode: unknown): SessionPermissionMode {
  return typeof mode === 'string' && SESSION_PERMISSION_MODES.has(mode)
    ? mode as SessionPermissionMode
    : 'ask';
}

function emitSessionPermissionMode(mode: unknown): SessionPermissionMode {
  const normalized = normalizeSessionPermissionMode(mode);
  useStore.getState().setSessionPermissionMode?.(normalized);
  window.dispatchEvent(new CustomEvent('hana-plan-mode', {
    detail: { enabled: normalized === 'read_only', mode: normalized },
  }));
  return normalized;
}

function findSessionProjection(path: string): any | null {
  return useStore.getState().sessions.find((session: any) => session.path === path) || null;
}

function isDeletedAgentSession(path: string): boolean {
  return findSessionProjection(path)?.agentDeleted === true;
}

function filterSessionScopedStateList(state: Record<string, any>, list: string[] | undefined, path: string): string[] {
  const current = Array.isArray(list) ? list : [];
  const key = sessionScopedKey(state, path) || path;
  return current.filter((item) => item !== key && item !== path);
}

function putSessionScopedStateListValue(state: Record<string, any>, list: string[] | undefined, path: string): string[] {
  const key = sessionScopedKey(state, path) || path;
  return [...filterSessionScopedStateList(state, list, path), key];
}

function reconcileStreamingSessionsForPath(
  state: Record<string, any>,
  streamingSessions: string[] | undefined,
  path: string,
  isStreaming: boolean,
): string[] {
  const current = Array.isArray(streamingSessions) ? streamingSessions : [];
  if (isStreaming) {
    return putSessionScopedStateListValue(state, current, path);
  }
  return filterSessionScopedStateList(state, current, path);
}

async function requestActiveSessionStreamResume(path: string, isStreaming: boolean): Promise<void> {
  if (!isStreaming) return;
  try {
    const { requestStreamResume } = await import('../services/stream-resume');
    requestStreamResume(path);
  } catch (err) {
    console.warn('[session] stream resume request skipped after switch:', err);
  }
}

async function resetDeskForSessionWorkspace({
  cwd,
  workspaceMountId,
  workspaceLabel,
}: {
  cwd?: string | null;
  workspaceMountId?: string | null;
  workspaceLabel?: string | null;
}): Promise<void> {
  // Session 切换后的 cwd 以服务端显式返回值为准；右侧 desk 视图归 workspace/CWD 所有。
  // 切到同一 workspace 时保留当前子目录；切到不同 workspace 时恢复该 workspace 的上次子目录。
  await activateWorkspaceDesk(cwd || null, {
    mountId: workspaceMountId || null,
    label: workspaceLabel || null,
  });
}

function clearSessionRuntimeCaches(path: string): void {
  useStore.getState().clearSession?.(path);
  // 终端 metadata 桶（terminalsBySession）也按会话清理，否则随归档数无界残留。
  useStore.getState().clearTerminals?.(path);
  useStore.setState((s: Record<string, any>) => {
    const attachedFilesBySession = deleteSessionScopedStateValue(s, s.attachedFilesBySession || {}, path);
    const sessionRegistryFilesByPath = deleteSessionScopedStateValue(s, s.sessionRegistryFilesByPath || {}, path);
    const drafts = deleteSessionScopedStateValue(s, s.drafts || {}, path);
    const draftDocs = deleteSessionScopedStateValue(s, s.draftDocs || {}, path);
    const activeSessionStreams = deleteSessionScopedStateValue(s, s.activeSessionStreams || {}, path);
    const computerOverlayBySession = deleteSessionScopedStateValue(s, s.computerOverlayBySession || {}, path);
    const scrollPositions = deleteSessionScopedStateValue(s, s.scrollPositions || {}, path);
    const sessionStreams = deleteSessionScopedStateValue(s, s.sessionStreams || {}, path);
    const browserBySession = deleteSessionScopedStateValue(s, s.browserBySession || {}, path);
    const todosBySession = deleteSessionScopedStateValue(s, s.todosBySession || {}, path);
    const todosLiveVersionBySession = deleteSessionScopedStateValue(s, s.todosLiveVersionBySession || {}, path);
    const todoPanelBySession = deleteSessionScopedStateValue(s, s.todoPanelBySession || {}, path);
    const todoPanelExpandedBySession = deleteSessionScopedStateValue(s, s.todoPanelExpandedBySession || {}, path);
    const sessionAuthorizedFoldersByPath = deleteSessionScopedStateValue(s, s.sessionAuthorizedFoldersByPath || {}, path);
    let inlineErrors = s.inlineErrors;
    if (inlineErrors) {
      inlineErrors = deleteSessionScopedStateValue(s, inlineErrors || {}, path);
      const key = sessionScopedKey(s, path) || path;
      inlineErrors = { ...inlineErrors, [key]: null, [path]: null };
    }
    return {
      attachedFilesBySession,
      sessionRegistryFilesByPath,
      drafts,
      draftDocs,
      sessionStreams,
      activeSessionStreams,
      browserBySession,
      computerOverlayBySession,
      scrollPositions,
      streamingSessions: filterSessionScopedStateList(s, s.streamingSessions || [], path),
      unreadOutputSessionPaths: filterSessionScopedStateList(s, s.unreadOutputSessionPaths || [], path),
      todosBySession,
      todosLiveVersionBySession,
      todoPanelBySession,
      todoPanelExpandedBySession,
      sessionAuthorizedFoldersByPath,
      capabilityRefreshingSessions: filterSessionScopedStateList(s, s.capabilityRefreshingSessions || [], path),
      inlineErrors,
    };
  });
}

// ══════════════════════════════════════════════════════
// 消息加载（从 app-messages-shim 迁移）
// ══════════════════════════════════════════════════════

/**
 * 从历史响应推导下一页游标（F1：分页边界只认服务端原始页面范围）。
 * - 新服务端：nextBefore（string=继续翻页；null=没有更早记录）；
 * - 旧服务端：回退用响应首条原始记录的 id（= 本页最早 display 序号），
 *   绝不用归并后的显示项 id（那是组内最后一条记录的序号，会让每页只推进 1 条）；
 * - undefined = 无法推导（无字段且无记录），调用方保持既有游标。
 */
function historyNextCursor(data: {
  nextBefore?: string | null;
  messages?: Array<{ id?: string } | undefined> | null;
}): string | null | undefined {
  if (typeof data.nextBefore === 'string') return data.nextBefore;
  if (data.nextBefore === null) return null;
  const firstId = Array.isArray(data.messages) ? data.messages[0]?.id : undefined;
  return typeof firstId === 'string' ? firstId : undefined;
}

export async function loadMessages(
  forPath?: string,
  opts?: { preloaded?: { data: any; etag: string | null; protocolHeader: string | null } },
): Promise<void> {
  const targetPath = forPath || useStore.getState().currentSessionPath;
  if (!targetPath) return;
  console.error('[lm-entry] targetPath=', targetPath.slice(-24), 'preloaded=', !!opts?.preloaded);
  const messageLiveVersionBefore = readMessageLiveVersion(targetPath);
  // 捕获 hydrate 前的 live 版本：若 fetch 期间有 tool_end 更新 todos，
  // 后面就跳过 hydrate 写入，避免旧快照覆盖刚收到的实时状态。
  const todosLiveVersionBefore =
    sessionScopedValue(useStore.getState() as Record<string, any>, useStore.getState().todosLiveVersionBySession, targetPath) ?? 0;
  // messages 维度的竞态护栏：rapid switch 或并发 load 时，只有最新一次调用
  // 的响应允许 apply initSession，stale 响应直接丢弃。
  const myVersion = useStore.getState().bumpLoadMessagesVersion(targetPath);
  // E06.3：协商页大小（能力未知 → null = 省略 limit，服务端默认 50）
  const negotiatedLimit = negotiatedHistoryPageLimit(targetPath);
  // SessionFile flight 记录（issue #2188）：hydrate 期间到达的 upsert / branch
  // reset 会被下面的 HTTP 快照整表覆盖。开一条 flight 记录桥接两者，hydrate
  // 通过后按 flight 结果决定是丢弃快照还是应用快照 + 重放 flight 期间的 upsert。
  useStore.getState().beginSessionFilesFlight(targetPath, myVersion);
  try {
    // E03 条件校验：仅当存在「已应用且未失效」的同页校验记录时发 If-None-Match
    //（记录不存在/预检失败 → 无条件请求，与原行为一致）。304 → 保留全部已加载
    // 状态，不解析 JSON、不 initSession、不推动游标。
    let data: any = opts?.preloaded?.data ?? null;
    let responseEtag: string | null = opts?.preloaded?.etag ?? null;
    let protocolHeader: string | null = opts?.preloaded?.protocolHeader ?? null;
    if (data == null) {
      const conditional = await conditionalMessagesFetch(targetPath, {
        sessionPath: targetPath,
        url: sessionMessagesUrl(targetPath),
        sessionId: sessionIdForPathFromState(useStore.getState() as Record<string, any>, targetPath),
        limit: 50,
        requestVersion: myVersion,
        appliedLiveVersion: messageLiveVersionBefore,
      });
      if (conditional.kind === 'not-modified') {
        // E03.3：304 全有效 → 只更新校验结果与结束状态；保留 messages/blocks/
        // todos/sessionFiles/hasMore/nextBefore/Run 状态与已加载更早页。
        useStore.getState().consumeSessionFilesFlight(targetPath, myVersion);
        return;
      }
      if (conditional.kind === 'superseded') {
        // 更新的 load 在途/目标已切换：丢弃本次（不向新目标补发旧请求）
        useStore.getState().consumeSessionFilesFlight(targetPath, myVersion);
        return;
      }
      data = await conditional.response.json();
      responseEtag = conditional.etag;
      protocolHeader = responseHeaderGet(conditional.response, 'lingxi-history-protocol');
    }
    const latestVersion =
      sessionScopedValue(useStore.getState() as Record<string, any>, useStore.getState()._loadMessagesVersion, targetPath) ?? 0;
    if (latestVersion !== myVersion) {
      // 已经有更新的 loadMessages 在途，stale 响应不应覆盖新状态。
      // todos 与 messages 必须作为同一份 hydrate 快照一起生效或一起丢弃。
      return;
    }
    // SessionFile hydrate（issue #2188）：必须放在 stale 检查之后、
    // messages/todos 的 live-version 早退检查之前——文件 registry 不受这两个
    // 护栏保护范围约束，否则 mid-flight 收到 live message/todo 更新时整次
    // hydrate 被放弃，registry 就会像 bugfix 前那样永远写不进去。
    // 只有 flight 期间没出现过 branch reset 才应用 HTTP 快照——reset 是全量权威
    // 替换，出现过就说明 registry 已经是权威状态（含 reset 后的 upsert），旧
    // 快照绝不能覆盖它。没有 reset 时应用快照后，重放 flight 期间记录的 upsert
    // （upsert 是逐文件权威最新，必须后写生效，恢复被整表快照覆盖掉的增量）。
    const flight = useStore.getState().consumeSessionFilesFlight(targetPath, myVersion);
    if (!flight || !flight.resetSeen) {
      useStore.getState().setSessionRegistryFiles(
        targetPath,
        Array.isArray(data.sessionFiles) ? data.sessionFiles : [],
      );
      for (const f of flight?.upserts ?? []) {
        useStore.getState().upsertSessionRegistryFile(targetPath, f);
      }
    }
    const messageLiveVersionNow = readMessageLiveVersion(targetPath);
    if (messageLiveVersionNow !== messageLiveVersionBefore) {
      console.log(
        '[loadMessages] 跳过 session hydrate: mid-flight 收到 live message 更新',
        targetPath,
      );
      return;
    }
    const todosLiveVersionNow =
      sessionScopedValue(useStore.getState() as Record<string, any>, useStore.getState().todosLiveVersionBySession, targetPath) ?? 0;
    if (todosLiveVersionNow !== todosLiveVersionBefore) {
      console.log(
        '[loadMessages] 跳过 session hydrate: mid-flight 收到 live todo 更新',
        targetPath,
      );
      return;
    }
    // per-session 清单面板快照：服务端权威 todoPanel 优先（含版本/收尾/收纳标志）；
    // 旧负载（仅 todos 数组）走防御性迁移，保持旧语义。
    const rawPanel = data.todoPanel && Array.isArray(data.todoPanel.todos) && data.todoPanel.todos.length > 0
      ? {
          todos: data.todoPanel.todos as TodoItem[],
          version: typeof data.todoPanel.version === 'string' ? data.todoPanel.version : null,
          finished: data.todoPanel.finished === true,
          allCompleted: data.todoPanel.allCompleted === true,
          dismissed: false,
          updateFailed: false,
        }
      : null;
    const rawTodos = data.todos || [];
    const migratedTodos = migrateLegacyTodos({ todos: rawTodos });
    // In-flight guard：流仍活跃时，磁盘上的最新 Run 必然不完整（jsonl 按模型轮落盘，
    // Run 未 settle），其历史投影不得携带终态（T06）；下方快照合并的 inflight 项
    // 才是该 Run 的实时代表。快照必须在投影前取，保证两者看到同一事实基线。
    const streamSnapshot = snapshotStreamBuffer(targetPath);
    const items = buildItemsFromHistory(data, { openTailRun: !!streamSnapshot?.hasContent });
    // 修订点 stamp：记录本次快照对应的磁盘修订点，后续 reconcile 与列表投影对比。
    const revision = typeof data.revision === 'string' ? data.revision : null;
    if (rawPanel) {
      useStore.getState().setSessionTodoPanel(targetPath, rawPanel);
    } else if (migratedTodos.length > 0) {
      // 旧负载：非空即活动清单（服务端已做生命周期投影）
      useStore.getState().setSessionTodoPanel(targetPath, {
        todos: migratedTodos,
        version: null,
        finished: false,
        allCompleted: false,
        dismissed: false,
        updateFailed: false,
      });
    } else {
      useStore.getState().setSessionTodoPanel(targetPath, null);
    }
    // hasMore 永远以服务端为准：items 为空（本页全被前端过滤的隐藏消息）不等于
    // 没有更早历史，静默截断会让旧消息永久不可达（T11b）。
    useStore.getState().initSession(
      targetPath,
      items,
      data.hasMore ?? false,
      revision,
      historyNextCursor(data),
    );
    if (items.length > 0 && targetPath === useStore.getState().currentSessionPath) {
      useStore.setState({ welcomeVisible: false });
    }
    // In-flight guard: jsonl 仅在 turn_end 落盘。若 session 在 stream 进行中
    // 被 reload（switchSession 冷启动 / stream-resume truncated），合并 buffer
    // 当前快照作为末尾 assistant，避免 UI 上"正在写的消息消失"。
    // 同步执行，不 await，保证中途不会有 text_delta 事件插入。
    const snapshot = streamSnapshot;
    if (snapshot?.hasContent) {
      useStore.getState().appendItem(targetPath, {
        type: 'message',
        data: buildInflightAssistantMessage(snapshot),
      });
    }
    console.error('[lm-dbg] before save, etag=', responseEtag, 'proto=', protocolHeader);
    // E03.2：initSession/todos/files 全部应用成功后原子保存校验记录
    //（etag/协议能力头缺失 → 不保存；覆盖证明=原始记录边界 id + 游标 + revision）。
    saveHistoryValidationRecord(targetPath, {
      sessionPath: targetPath,
      url: sessionMessagesUrl(targetPath, negotiatedLimit != null ? { limit: String(negotiatedLimit) } : {}),
      sessionId: sessionIdForPathFromState(useStore.getState() as Record<string, any>, targetPath),
      limit: negotiatedLimit ?? 50,
      appliedLiveVersion: messageLiveVersionBefore,
      etag: responseEtag,
      protocolHeader,
      data,
      todosVersion: todosLiveVersionBefore,
    });
  } catch (err) {
    console.error('[loadMessages] error:', err);
    // fetch 失败也要清理本次 flight 记录，避免残留记录被后续 load 误判 version 冲突
    // （不消费返回值：失败路径不需要重放任何东西）。
    useStore.getState().consumeSessionFilesFlight(targetPath, myVersion);
  }
}

interface TodoPanelPayload {
  todos?: TodoItem[];
  version?: string | null;
  finished?: boolean;
  allCompleted?: boolean;
  dismissed?: boolean;
  removed?: boolean;
}

function panelFromPayload(payload: TodoPanelPayload | null | undefined) {
  if (!payload || !Array.isArray(payload.todos) || payload.todos.length === 0) return null;
  if (payload.removed === true || payload.dismissed === true) return null;
  return {
    todos: payload.todos,
    version: typeof payload.version === 'string' ? payload.version : null,
    finished: payload.finished === true,
    allCompleted: payload.allCompleted === true,
    dismissed: false,
    updateFailed: false,
  };
}

function applyTodoActionResponse(sessionPath: string, payload: TodoPanelPayload | null | undefined): void {
  useStore.getState().setSessionTodoPanel(sessionPath, panelFromPayload(payload));
  useStore.getState().bumpTodosLiveVersion(sessionPath);
}

function presentTodoActionError(err: unknown): void {
  const presented = presentError(err);
  useStore.getState().addToast(
    presented.text,
    'error',
    6000,
    presented.code ? { errorCode: presented.code } : undefined,
  );
}

/**
 * 清单已在他处更新（版本失配）：旧版本操作被识别并拒绝，
 * 提示刷新，不误改新加入的任务（A17）。原清单保持不变。
 */
function presentTodoVersionMismatch(): void {
  const translate = window.t ?? ((key: string) => key);
  useStore.getState().addToast(translate('todoPanel.versionMismatch'), 'error', 6000, {
    errorCode: 'todo_version_mismatch',
  });
}

async function postTodoAction(
  sessionPath: string,
  endpoint: string,
  version: string | null,
): Promise<boolean> {
  const res = await lingxiFetch(endpoint, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path: sessionPath, version }),
    throwOnHttpError: false,
  });
  if (res.status === 409) {
    const data = await res.json().catch(() => null);
    if (data?.code === 'todo_version_mismatch') {
      presentTodoVersionMismatch();
      return false;
    }
    throw errorWithCode(data?.error || `todo action conflict (${res.status})`, data?.code || 'todo_conflict');
  }
  if (!res.ok) {
    const data = await res.json().catch(() => null);
    throw errorWithCode(data?.error || `todo action failed (${res.status})`, data?.code || 'todo_action_failed');
  }
  const data = await res.json().catch(() => null);
  applyTodoActionResponse(sessionPath, data?.panel ?? data);
  return true;
}

/**
 * 用户确认剩余任务已完成：未完成项改为已完成，已取消项保持取消。
 * 携带用户看见的那一版清单版本；服务端版本失配会拒绝（409），
 * 不把新加入的任务一起完成（A17）。
 */
export async function completeSessionTodos(sessionPath: string): Promise<boolean> {
  if (!sessionPath) return false;
  const state = useStore.getState();
  if (sessionScopedListIncludes(state as Record<string, any>, state.streamingSessions, sessionPath)) return false;

  const panel = sessionScopedValue(state as Record<string, any>, state.todoPanelBySession, sessionPath);
  try {
    return await postTodoAction(sessionPath, '/api/sessions/todos/complete', panel?.version ?? null);
  } catch (err) {
    presentTodoActionError(err);
    return false;
  }
}

/**
 * 取消剩余任务：待开始/进行中/受阻项改为已取消，已完成项保持完成。
 * 只改变这份计划，不终止终端进程、工作流或其他后台任务。
 */
export async function cancelSessionTodos(sessionPath: string): Promise<boolean> {
  if (!sessionPath) return false;
  const state = useStore.getState();
  if (sessionScopedListIncludes(state as Record<string, any>, state.streamingSessions, sessionPath)) return false;

  const panel = sessionScopedValue(state as Record<string, any>, state.todoPanelBySession, sessionPath);
  try {
    return await postTodoAction(sessionPath, '/api/sessions/todos/cancel', panel?.version ?? null);
  } catch (err) {
    presentTodoActionError(err);
    return false;
  }
}

/**
 * 收纳已结束清单：只改变当前展示（隐藏收尾摘要），不改完成/取消结果，
 * 历史记录保留。重开会话不重新弹出已收纳摘要。
 */
export async function dismissSessionTodoPanel(sessionPath: string): Promise<boolean> {
  if (!sessionPath) return false;
  const state = useStore.getState();
  const panel = sessionScopedValue(state as Record<string, any>, state.todoPanelBySession, sessionPath);
  try {
    const res = await lingxiFetch('/api/sessions/todos/dismiss', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: sessionPath, version: panel?.version ?? null }),
      throwOnHttpError: false,
    });
    if (res.status === 409) {
      const data = await res.json().catch(() => null);
      if (data?.code === 'todo_version_mismatch') {
        presentTodoVersionMismatch();
        return false;
      }
    }
    if (!res.ok) {
      const data = await res.json().catch(() => null);
      throw errorWithCode(data?.error || `todo dismiss failed (${res.status})`, data?.code || 'todo_action_failed');
    }
    useStore.getState().setSessionTodoPanel(sessionPath, null);
    useStore.getState().bumpTodosLiveVersion(sessionPath);
    return true;
  } catch (err) {
    presentTodoActionError(err);
    return false;
  }
}

function buildInflightAssistantMessage(snap: StreamBufferSnapshot): ChatMessage {
  if (snap.blocks?.length) {
    return {
      id: snap.messageId || `inflight-${Date.now()}`,
      role: 'assistant',
      blocks: snap.blocks,
      timestamp: Date.now(),
    };
  }
  const blocks: ContentBlock[] = [];
  if (snap.thinking || snap.inThinking) {
    blocks.push({ type: 'thinking', content: snap.thinking, sealed: !snap.inThinking });
  }
  if (snap.mood) {
    blocks.push({ type: 'mood', yuan: snap.moodYuan, text: snap.mood });
  }
  if (snap.text) {
    const displayText = snap.text.replace(/<tool_code>[\s\S]*?<\/tool_code>\s*/g, '');
    blocks.push({ type: 'text', source: displayText });
  }
  return { id: snap.messageId || `inflight-${Date.now()}`, role: 'assistant', blocks, timestamp: Date.now() };
}

/** 上滑加载更早的消息（分页） */
export async function loadMoreMessages(forPath?: string): Promise<void> {
  const targetPath = forPath || useStore.getState().currentSessionPath;
  if (!targetPath) return;
  const session = sessionScopedValue(useStore.getState() as Record<string, any>, useStore.getState().chatSessions, targetPath);
  if (!session || !session.hasMore || session.loadingMore) return;

  useStore.getState().setLoadingMore(targetPath, true);
  try {
    // 游标（F1）：优先服务端 nextBefore；旧服务端回退 oldestId（hydrate 时已按
    // 原始首条记录 id 写入）。两者都是原始 display 序号，不是归并显示项身份。
    const before = session.nextBefore ?? session.oldestId ?? '';
    const moreLimit = negotiatedHistoryPageLimit(targetPath);
    const res = await lingxiFetch(sessionMessagesUrl(targetPath, { before, ...(moreLimit != null ? { limit: String(moreLimit) } : {}) }));
    const data = await res.json();
    if (Array.isArray(data.sessionFiles)) {
      useStore.getState().setSessionRegistryFiles(targetPath, data.sessionFiles);
    }
    const items = buildItemsFromHistory(data);
    const cursor = historyNextCursor(data);
    let hasMore = data.hasMore ?? false;
    if (hasMore && typeof cursor !== 'string') {
      // 服务端声称还有更早记录却给不出可推进的游标（旧服务端空页/协议异常）：
      // 不能静默截断，也不能原地死循环——显式记录诊断并终止翻页，等待下次
      // 全量 hydrate 修正。
      console.error('[loadMoreMessages] 分页游标缺失，终止翻页等待重新 hydrate:', targetPath, before);
      hasMore = false;
    }
    // 空页（全部被前端过滤 / 边界页）也要推进分页进度与 hasMore（T11b），
    // prependItems 负责幂等合并与游标落盘。
    useStore.getState().prependItems(targetPath, items, hasMore, cursor);
  } catch (err) {
    console.error('[loadMoreMessages] error:', err);
    useStore.getState().setLoadingMore(targetPath, false);
  }
}

// ══════════════════════════════════════════════════════
// 会话修订点校验补拉（issue #1610）
// ══════════════════════════════════════════════════════

// per-session in-flight 去重：focus / online / WS reconnect 等触发器可能同时到达，
// 同一会话同一时刻最多一个补拉请求在途。
const _revisionReconcileInFlight = new Map<string, Promise<void>>();

/**
 * E03.4：同 revision 既有触发点上的条件重校验（不新增轮询/预取；调用方即
 * reconcileCurrentSessionMessages 的既有触发链：返回会话/刷新/重连/移动端前台）。
 * 304 → 状态保留；200 → 交给 loadMessages 既有完整归并链（preloaded，单次传输）。
 */
async function revalidateCurrentSessionConditional(
  path: string,
  reason: string,
  listRevision: string,
): Promise<void> {
  try {
    const outcome = await conditionalMessagesFetch(path, {
      sessionPath: path,
      url: sessionMessagesUrl(path),
      sessionId: sessionIdForPathFromState(useStore.getState() as Record<string, any>, path),
      limit: 50,
      requestVersion: useStore.getState()._loadMessagesVersion?.[path] ?? 0,
      appliedLiveVersion: readMessageLiveVersion(path),
    });
    if (outcome.kind !== 'ok') return; // 304：状态保留；superseded：已有更新流程在途
    const data = await outcome.response.json();
    if (!Array.isArray(data?.messages)) return; // 非 JSON/异常响应：按现有错误流程静默降级
    await loadMessages(path, {
      preloaded: {
        data,
        etag: outcome.etag,
        protocolHeader: outcome.response.headers.get('lingxi-history-protocol'),
      },
    });
  } catch (err) {
    console.warn(`[session] conditional revalidate failed (${reason}):`, err);
  }
}

/**
 * 校验「当前打开会话」的缓存内容是否落后于磁盘真相，落后则补拉。
 *
 * 修订点对比：chatSessions[path].revision（hydrate 时 stamp 的 stat 签名）
 * vs store.sessions 列表投影的 revision（最近一次列表刷新看到的磁盘状态）。
 * 两者不一致说明磁盘在本端没有消费到的窗口内前进过——典型场景是 Bridge /rc
 * 接管期间 web/mobile 端 WS 断连（手机锁屏），live 事件全部丢失。
 *
 * 边界（刻意保守，宁可少拉）：
 *   - 只处理当前打开的会话；后台会话切换回来时由 switchSession 触发同一校验
 *   - 流式进行中不补拉：live 事件流正在喂内容，turn 结束后列表刷新会再触发
 *   - 列表投影无 revision（老服务端 / 内存占位）不盲拉
 *   - 缓存 revision 为 null（未知，如 WS 端新会话的空 init）且列表有 revision
 *     时补拉一次，把修订点 stamp 上
 *
 * 调用方约定：在「拿到新鲜列表之后」调用（loadSessions / loadMobileSessions 之后），
 * 否则对比的是旧投影，没有意义。
 */
export function reconcileCurrentSessionMessages(reason = 'unknown'): Promise<void> | undefined {
  const s = useStore.getState();
  const target = s.currentSessionPath;
  if (!target || s.pendingNewSession || s.pendingSessionSwitchPath) return undefined;
  if (sessionScopedListIncludes(s as Record<string, any>, s.streamingSessions || [], target)) return undefined;
  const cached = sessionScopedValue(s as Record<string, any>, s.chatSessions, target);
  if (!cached) return undefined; // 冷启动 / 切换路径负责首载
  const projection = s.sessions.find((session) => session.path === target);
  const listRevision = typeof projection?.revision === 'string' ? projection.revision : null;
  if (!listRevision) return undefined;
  if ((cached.revision ?? null) === listRevision) {
    // E03.4：既有真实触发点（返回会话/刷新/重连后拿到新鲜列表）上的条件校验。
    // revision 相同 ≠ 外部状态相同（todos/files/表示变化不反映在文件 stat 上）：
    // 一次条件请求；304 → 状态保留（不触发完整 hydrate）；200 → 表示已变，
    // 交给既有 loadMessages 完整归并链（单次传递，不再补发第二个请求）。
    if (hasMessagesValidationRecord(target, sessionIdForPathFromState(useStore.getState() as Record<string, any>, target), 50)) {
      void revalidateCurrentSessionConditional(target, reason, listRevision);
    }
    return undefined;
  }

  const existing = _revisionReconcileInFlight.get(target);
  if (existing) return existing;

  const inFlight = loadMessages(target)
    .catch((err) => {
      // loadMessages 内部已兜底，这里只防御未来改动让异常冒出导致 in-flight 卡死。
      console.warn(`[session] revision reconcile failed (${reason}):`, err);
    })
    .finally(() => {
      _revisionReconcileInFlight.delete(target);
    });
  _revisionReconcileInFlight.set(target, inFlight);
  return inFlight;
}

// ══════════════════════════════════════════════════════
// Session 列表
// ══════════════════════════════════════════════════════

// 与 /api/sessions 并行探测 session 元数据待恢复状态（/api/health 的
// sessionStore 附块）。失败/超时一律静默忽略——这只是一个附加提示信号，绝不能
// 让健康检查探测的失败反过来拖垮或延后会话列表本身的加载。
async function fetchSessionMetaRecoveryStatus(): Promise<SessionMetaRecoveryStatus | null> {
  try {
    const res = await lingxiFetch('/api/health');
    const data = await res.json();
    return (data && typeof data === 'object' && data.sessionStore) || null;
  } catch {
    return null;
  }
}

export async function loadSessions(): Promise<void> {
  // 先发起 /api/sessions（调用顺序上排在前面，保持它是 loadSessions() 触发的
  // 第一个 lingxiFetch 调用——调用方/测试对"/api/sessions 是这次加载的第一个
  // 请求"这个顺序有既有假设），紧接着不等待地发起 /api/health 探测：两个请求
  // 仍然背靠背同时打到网络上，只是调用顺序固定，不会因为并行而变得不确定。
  const sessionsFetchPromise = lingxiFetch('/api/sessions');
  const metaRecoveryPromise = fetchSessionMetaRecoveryStatus();
  try {
    const res = await sessionsFetchPromise;
    const data = await res.json();
    const serverSessions = Array.isArray(data) ? data.map(normalizeServerSessionProjection) : [];
    const localSessions = useStore.getState().sessions || [];
    const sessions = mergeSessionsWithOptimisticFirstMessages(serverSessions, localSessions);

    useStore.setState((state: any) => {
      const sessionLocatorsById = mergeSessionLocators(state.sessionLocatorsById || {}, sessions);
      const currentSessionId = typeof state.currentSessionId === 'string' && state.currentSessionId.trim()
        ? state.currentSessionId.trim()
        : null;
      const currentLocatorPath = currentSessionId
        && !state.pendingNewSession
        && !state.pendingSessionSwitchPath
        ? sessionLocatorsById[currentSessionId]?.path || null
        : null;
      return {
        sessions,
        sessionLocatorsById,
        ...(currentLocatorPath && currentLocatorPath !== state.currentSessionPath
          ? { currentSessionPath: currentLocatorPath }
          : {}),
      };
    });

    // 冷启动循环状态：用最新会话列表重建 loopStatusBySession（后端只对 running/paused 注入）。
    // 运行时变更靠 loop_status WS 增量；这里每次重载都用列表快照替换，避免已停止的循环残留。
    // 字段归一化与 WS 路径（ws-message-handler loop_status）保持一致，避免两种来源写入不同形状。
    const nextLoopStatusBySession: Record<string, any> = {};
    for (const s of serverSessions) {
      if (!s || !s.sessionId || !s.loopStatus) continue;
      nextLoopStatusBySession[s.sessionId] = {
        status: s.loopStatus.status,
        turnCount: s.loopStatus.turnCount ?? 0,
        maxTurns: s.loopStatus.maxTurns ?? null,
        pausedReason: s.loopStatus.pausedReason ?? null,
        prompt: s.loopStatus.prompt ?? null,
      };
    }
    useStore.setState({ loopStatusBySession: nextLoopStatusBySession });

    const latest = useStore.getState();
    if (
      sessions.length > 0
      && !latest.currentSessionPath
      && !latest.pendingNewSession
      && !latest.pendingSessionSwitchPath
    ) {
      // 首次加载：走完整的 switchSession 确保后端同步 + 消息加载
      await switchSession(sessions[0].path);
    }
    // 列表投影刷新后校验当前会话缓存修订点：缓存 revision 落后（如新建会话首条消息
    // 尚未被本端 WS 消费）时自动补拉。此前桌面端仅 chat-find-locate / 移动端前台触发
    // reconcile，发送后的 loadSessions 刷新拿到了新 revision 却没有消费者——
    // 「列表说磁盘前进了、缓存永远不追」的缺口在此闭合（issue #1610 桌面端补齐）。
    void reconcileCurrentSessionMessages('sessions_refresh');
  } catch { /* ignore */ }
  useStore.getState().setSessionMetaRecovery(await metaRecoveryPromise);
}

const EMPTY_FIRST_MESSAGE_PLACEHOLDER = '(no messages)';

function nonPlaceholderText(value: unknown): string {
  if (typeof value !== 'string') return '';
  const trimmed = value.trim();
  return trimmed === EMPTY_FIRST_MESSAGE_PLACEHOLDER ? '' : trimmed;
}

function normalizeServerSessionProjection(session: any): any {
  if (!session || typeof session !== 'object') return session;
  if (session.firstMessage === EMPTY_FIRST_MESSAGE_PLACEHOLDER) {
    return { ...session, firstMessage: '' };
  }
  return session;
}

function withoutOptimisticFirstMessageMarker(session: any): any {
  if (!session || typeof session !== 'object') return session;
  if (!session._optimisticFirstMessage) return session;
  const { _optimisticFirstMessage, ...rest } = session;
  return rest;
}

function isOptimisticFirstMessageProjection(session: any): boolean {
  return !!(session && session._optimisticFirstMessage && Number(session.messageCount || 0) > 0);
}

function serverProjectionHasPersistedContent(session: any): boolean {
  return Number(session?.messageCount || 0) > 0
    || !!nonPlaceholderText(session?.firstMessage)
    || !!nonPlaceholderText(session?.title);
}

function shouldKeepOptimisticFirstMessage(serverSession: any, localSession: any): boolean {
  return isOptimisticFirstMessageProjection(localSession)
    && !serverProjectionHasPersistedContent(serverSession);
}

function mergeSessionsWithOptimisticFirstMessages(serverSessions: any[], localSessions: any[]): any[] {
  const localByPath = new Map<string, any>();
  for (const session of localSessions) {
    if (typeof session?.path === 'string' && isOptimisticFirstMessageProjection(session)) {
      localByPath.set(session.path, session);
    }
  }
  if (localByPath.size === 0) return serverSessions.map(withoutOptimisticFirstMessageMarker);

  const seenPaths = new Set<string>();
  const merged = serverSessions.map((serverSession) => {
    const path = typeof serverSession?.path === 'string' ? serverSession.path : null;
    if (!path) return withoutOptimisticFirstMessageMarker(serverSession);
    seenPaths.add(path);
    const localSession = localByPath.get(path);
    if (!shouldKeepOptimisticFirstMessage(serverSession, localSession)) {
      return withoutOptimisticFirstMessageMarker(serverSession);
    }
    return {
      ...localSession,
      ...serverSession,
      firstMessage: nonPlaceholderText(localSession.firstMessage),
      messageCount: Math.max(Number(localSession.messageCount || 0), 1),
      modified: localSession.modified,
      _optimisticFirstMessage: true,
    };
  });

  const localOnly = Array.from(localByPath.values()).filter((session) => !seenPaths.has(session.path));
  return [...localOnly, ...merged];
}

export function upsertOptimisticSessionFirstMessage(
  sessionPath: string | null | undefined,
  messageText: string,
  timestamp = new Date().toISOString(),
): void {
  const path = typeof sessionPath === 'string' && sessionPath.trim() ? sessionPath : null;
  if (!path) return;

  useStore.setState((state: any) => {
    const sessions = Array.isArray(state.sessions) ? state.sessions : [];
    const existingIndex = sessions.findIndex((session: any) => session?.path === path);
    const existing = existingIndex >= 0 ? sessions[existingIndex] : null;
    if (existing && !isOptimisticFirstMessageProjection(existing) && serverProjectionHasPersistedContent(existing)) {
      return {};
    }
    const sessionId = normalizeSessionId(existing?.sessionId)
      || (state.currentSessionPath === path ? normalizeSessionId(state.currentSessionId) : null)
      || sessionIdForPathFromState(state, path);
    const firstMessage = nonPlaceholderText(existing?.firstMessage) || nonPlaceholderText(messageText);
    const messageCount = Math.max(Number(existing?.messageCount || 0), 1);
    const optimisticProjection = {
      ...(existing || {}),
      path,
      ...(sessionId ? { sessionId } : {}),
      agentId: existing?.agentId ?? state.currentAgentId ?? state.selectedAgentId ?? null,
      agentName: existing?.agentName ?? state.agentName ?? '',
      cwd: existing?.cwd ?? state.deskBasePath ?? state.selectedFolder ?? '',
      projectId: existing?.projectId ?? state.pendingProjectId ?? null,
      workspaceMountId: existing?.workspaceMountId ?? state.deskWorkspaceMountId ?? state.selectedWorkspaceMountId ?? null,
      workspaceLabel: existing?.workspaceLabel ?? state.deskWorkspaceLabel ?? state.selectedWorkspaceLabel ?? null,
      firstMessage,
      messageCount,
      modified: timestamp,
      created: existing?.created ?? timestamp,
      _optimisticFirstMessage: true,
    };
    const nextSessions = existingIndex >= 0
      ? sessions.map((session: any, index: number) => (index === existingIndex ? optimisticProjection : session))
      : [optimisticProjection, ...sessions];
    return {
      sessions: nextSessions,
      sessionLocatorsById: mergeSessionLocators(state.sessionLocatorsById || {}, nextSessions),
    };
  });
}

// ══════════════════════════════════════════════════════
// Session 切换
// ══════════════════════════════════════════════════════

export async function switchSession(path: string): Promise<void> {
  const s = useStore.getState();
  const myVersion = ++_switchVersion;
  _switchAbortController?.abort();
  _switchAbortController = null;

  if (path === s.currentSessionPath && !s.pendingNewSession) {
    useStore.setState(state => ({
      pendingSessionSwitchPath: null,
      unreadOutputSessionPaths: filterSessionScopedStateList(state as Record<string, any>, state.unreadOutputSessionPaths || [], path),
    }));
    return;
  }

  useStore.getState().clearStaleMessageLocate(path);
  useStore.setState({ pendingSessionSwitchPath: path });

  if (isDeletedAgentSession(path)) {
    await switchDeletedAgentSession(path, myVersion);
    return;
  }

  // 关闭浮动面板
  const activePanel = useStore.getState().activePanel;
  if (activePanel === 'activity' || activePanel === 'automation') {
    useStore.getState().setActivePanel(null);
  }

  const abortController = new AbortController();
  _switchAbortController = abortController;
  const targetSessionId = sessionIdForPathFromState(s as Record<string, any>, path);

  try {
    const res = await lingxiFetch('/api/sessions/switch', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        path,
        ...(targetSessionId ? { sessionId: targetSessionId } : {}),
        currentSessionPath: s.currentSessionPath,
      }),
      signal: abortController.signal,
    });
    const data = await res.json();
    if (!isCurrentSwitch(myVersion, path)) return;
    if (data.error) {
      // 带上错误码，呈现层才能把它翻成人话；没有码的原生崩溃走兜底文案 + 详情。
      const routeError = normalizeSessionRouteError(data);
      console.error('[session] switch failed:', routeError.message, routeError.code || '');
      useStore.setState({ pendingSessionSwitchPath: null });
      showSessionSwitchError(path, errorWithCode(routeError.message, routeError.code));
      return;
    }

    const state = useStore.getState();

    // 以服务端事实对齐当前 session 的流式状态。刷新或重连后，renderer 的本地集合可能已经过期。
    const isStreaming = data.isStreaming === true;
    const streamingSessions = reconcileStreamingSessionsForPath(state as Record<string, any>, state.streamingSessions, path, isStreaming);
    const activeSessionStreams = { ...(state.activeSessionStreams || {}) };
    const activeStreamKey = sessionScopedKey(state as Record<string, any>, path) || path;
    if (isStreaming) {
      activeSessionStreams[activeStreamKey] = activeSessionStreams[activeStreamKey]
        || activeSessionStreams[path]
        || { streamId: null, turnId: null };
      if (activeStreamKey !== path) delete activeSessionStreams[path];
    } else {
      delete activeSessionStreams[activeStreamKey];
      delete activeSessionStreams[path];
    }

    // 同步全局 agent 上下文
    const switchedAgent = data.agentId && data.agentId !== state.currentAgentId;
    const agentPatch: Record<string, any> = {};

    if (switchedAgent) {
      const ag = state.agents.find((a: any) => a.id === data.agentId);
      agentPatch.currentAgentId = data.agentId;
      agentPatch.agentName = data.agentName || ag?.name || data.agentId;
      agentPatch.agentYuan = ag?.yuan || 'lingxi';
      agentPatch.agentAvatarUrl = ag?.hasAvatar ? lingxiUrl(`/api/agents/${data.agentId}/avatar?t=${Date.now()}`) : null;
      agentPatch.homeFolder = typeof ag?.homeFolder === 'string' && ag.homeFolder.trim()
        ? ag.homeFolder.trim()
        : null;
    }

    // 保存当前 session 的附件到 keyed store
    const currentPath = s.currentSessionPath;
    const currentAttachments = state.attachedFiles;
    if (currentPath) {
      useStore.setState(prev => ({
        attachedFilesBySession: putSessionScopedStateValue(
          prev as Record<string, any>,
          prev.attachedFilesBySession || {},
          currentPath,
          [...currentAttachments],
        ),
      }));
    }

    // 在设置 currentSessionPath 之前预加载消息历史。
    // 一旦 currentSessionPath 指向新 session，主窗口 WebSocket 会将该 session 的流式事件
    // 路由到 streamBufferManager，触发 bumpMessageLiveVersion，导致 loadMessages 的
    // 竞态守卫跳过 hydrate，store 丢失完整历史。提前加载可避免此竞态。
    const hasData = !!sessionScopedValue(useStore.getState() as Record<string, any>, useStore.getState().chatSessions, path);
    if (!hasData) {
      await loadMessages(path);
      if (myVersion !== _switchVersion) return;
    }

    // 批量更新 store（切 currentSessionPath 切换对话内容；可见 desk/preview 状态由 workspace 激活流程恢复）
    useStore.setState((prev: any) => ({
      ...currentSessionIdentityPatch(prev, path, data.sessionId),
      pendingSessionSwitchPath: null,
      pendingNewSession: false,
      pendingDraftId: null,
      pendingProjectId: null,
      pendingNewSessionThinkingLevel: null,
      pendingNewSessionPermissionMode: null,
      selectedFolder: null,
      selectedWorkspaceMountId: null,
      selectedWorkspaceLabel: null,
      workspaceFolders: Array.isArray(data.workspaceFolders) ? data.workspaceFolders : [],
      sessionAuthorizedFoldersByPath: {
        ...putSessionScopedStateValue(
          state,
          state.sessionAuthorizedFoldersByPath || {},
          path,
          Array.isArray(data.authorizedFolders) ? data.authorizedFolders : [],
        ),
      },
      selectedAgentId: null,
      welcomeVisible: false,
      memoryEnabled: data.memoryEnabled !== false,
      streamingSessions,
      activeSessionStreams,
      unreadOutputSessionPaths: filterSessionScopedStateList(state as Record<string, any>, state.unreadOutputSessionPaths || [], path),
      attachedFiles: sessionScopedValue(state as Record<string, any>, state.attachedFilesBySession || {}, path) || [],
      deskContextAttached: false,
      docContextAttached: false,
      ...agentPatch,
    }));

    // 缓存命中跳过了 loadMessages 时，校验修订点：会话在后台期间（如 Bridge /rc
    // 接管 + 本端 WS 断连）磁盘可能已前进，缓存不能直接当真相（issue #1610）。
    // fire-and-forget：先呈现缓存内容，补拉结果通过 store 更新自然落地。
    if (hasData) {
      void reconcileCurrentSessionMessages('session_switch');
    }

    await resetDeskForSessionWorkspace({
      cwd: data.cwd || null,
      workspaceMountId: data.workspaceMountId || null,
      workspaceLabel: data.workspaceLabel || null,
    });
    if (myVersion !== _switchVersion) return;

    // 同步浏览器状态到 keyed store（服务端返回当前 session 的 browser 状态）
    if (path) {
      setBrowserStateForPath(path, {
        running: !!data.browserRunning,
        url: data.browserUrl || null,
        thumbnail: data.browserRunning ? (browserStateForPath(state as any, path).thumbnail ?? null) : null,
      });
    }

    useStore.getState().clearQuotedSelection();

    emitSessionPermissionMode(data.permissionMode || data.accessMode);
    if (data.thinkingLevel) {
      useStore.getState().setThinkingLevel(data.thinkingLevel);
    }

    // 刷新模型列表（当前 session 的模型可能不同）
    loadModels();

    // Hydrate per-session model snapshot from switch response。
    // provider 缺失不写入——空 provider 会让 ModelSelector 的复合键匹配全错
    // （老 session 的 meta 可能没带 provider，走 migration 或下一次显式选择修复）。
    if (data.currentModelId && data.currentModelProvider) {
      useStore.getState().updateSessionModel(path, {
        id: data.currentModelId,
        name: data.currentModelName || data.currentModelId,
        provider: data.currentModelProvider,
        input: Array.isArray(data.currentModelInput) ? data.currentModelInput : undefined,
        video: data.currentModelVideo ?? undefined,
        videoTransport: data.currentModelVideoTransport ?? undefined,
        videoTransportSupported: data.currentModelVideoTransportSupported ?? undefined,
        audio: data.currentModelAudio ?? undefined,
        audioTransport: data.currentModelAudioTransport ?? undefined,
        audioTransportSupported: data.currentModelAudioTransportSupported ?? undefined,
        reasoning: data.currentModelReasoning ?? undefined,
        xhigh: data.currentModelXhigh ?? undefined,
        thinkingLevels: Array.isArray(data.currentModelThinkingLevels) ? data.currentModelThinkingLevels : undefined,
        defaultThinkingLevel: data.currentModelDefaultThinkingLevel ?? undefined,
        contextWindow: data.currentModelContextWindow ?? undefined,
        available: data.currentModelAvailable !== false,
        unavailableReason: data.currentModelAvailable === false
          ? (data.currentModelUnavailableReason || 'temporarily_unavailable')
          : null,
      });
    }

    await requestActiveSessionStreamResume(path, isStreaming);
    if (myVersion !== _switchVersion) return;

    // 切换会话后刷新 context ring
    useStore.setState({ contextTokens: null, contextWindow: null, contextPercent: null });
    import('../services/websocket').then(({ getWebSocket, requestTerminalSnapshotForCurrentSession }) => {
      // 终端快照走会话切换的通用路径：compact 布局不挂 TerminalCard，没人替它发请求。
      // 服务端快照是幂等单播，与 ws 重连 / TerminalCard 挂载时的重复请求不冲突。
      requestTerminalSnapshotForCurrentSession(useStore.getState());
      const wsConn = getWebSocket();
      if (wsConn?.readyState === WebSocket.OPEN) {
        const sessionId = sessionIdForPathFromState(useStore.getState() as Record<string, any>, path);
        wsConn.send(JSON.stringify({
          type: 'context_usage',
          sessionPath: path,
          ...(sessionId ? { sessionId } : {}),
        }));
      }
    }).catch((err) => {
      console.warn('[session] context usage refresh skipped:', err);
    });

    // Restore input focus only if the user is still in the chat surface that initiated the switch.
    requestChatInputFocus(path);
  } catch (err) {
    if (myVersion !== _switchVersion || isAbortError(err)) return;
    useStore.setState((state: Record<string, any>) => (
      state.pendingSessionSwitchPath === path ? { pendingSessionSwitchPath: null } : {}
    ));
    console.error('[session] switch failed:', err);
    showSessionSwitchError(path, err);
  } finally {
    if (_switchAbortController === abortController) {
      _switchAbortController = null;
    }
  }
}

async function switchDeletedAgentSession(path: string, version: number): Promise<void> {
  const state = useStore.getState();
  const projection = findSessionProjection(path);
  const currentPath = state.currentSessionPath;
  const currentAttachments = state.attachedFiles;
  if (currentPath) {
    useStore.setState(prev => ({
      attachedFilesBySession: putSessionScopedStateValue(
        prev as Record<string, any>,
        prev.attachedFilesBySession || {},
        currentPath,
        [...currentAttachments],
      ),
    }));
  }

  useStore.setState({
    ...currentSessionIdentityPatch(state as Record<string, any>, path, projection?.sessionId),
    currentSessionPath: path,
    pendingSessionSwitchPath: null,
    pendingNewSession: false,
    pendingDraftId: null,
    pendingProjectId: null,
    selectedFolder: null,
    selectedWorkspaceMountId: null,
    selectedWorkspaceLabel: null,
    workspaceFolders: [],
    sessionAuthorizedFoldersByPath: {
      ...putSessionScopedStateValue(state, state.sessionAuthorizedFoldersByPath || {}, path, []),
    },
    selectedAgentId: null,
    welcomeVisible: false,
    streamingSessions: filterSessionScopedStateList(state as Record<string, any>, state.streamingSessions, path),
    activeSessionStreams: Object.fromEntries(
      Object.entries(state.activeSessionStreams || {}).filter(([sessionPath]) => {
        const key = sessionScopedKey(state as Record<string, any>, path) || path;
        return sessionPath !== key && sessionPath !== path;
      }),
    ),
    unreadOutputSessionPaths: filterSessionScopedStateList(state as Record<string, any>, state.unreadOutputSessionPaths || [], path),
    attachedFiles: sessionScopedValue(state as Record<string, any>, state.attachedFilesBySession || {}, path) || [],
    deskContextAttached: false,
    docContextAttached: false,
  });

  await resetDeskForSessionWorkspace({
    cwd: projection?.cwd || null,
    workspaceMountId: (projection as any)?.workspaceMountId || null,
    workspaceLabel: (projection as any)?.workspaceLabel || null,
  });
  if (version !== _switchVersion) return;

  useStore.getState().clearQuotedSelection();
  emitSessionPermissionMode('read_only');

  const hasData = !!sessionScopedValue(useStore.getState() as Record<string, any>, useStore.getState().chatSessions, path);
  if (!hasData) {
    await loadMessages(path);
  }
}

// ══════════════════════════════════════════════════════
// 新建 Session
// ══════════════════════════════════════════════════════

interface CreateNewSessionOptions {
  projectId?: string | null;
  cwd?: string | null;
}

type PendingSessionCreateBody = Record<string, any>;

function buildPendingSessionCreateBody(state: Record<string, any>): PendingSessionCreateBody {
  const body: PendingSessionCreateBody = {
    memoryEnabled: state.memoryEnabled,
    recordWorkspaceHistory: true,
  };
  if (state.selectedWorkspaceMountId) {
    body.workspaceMountId = state.selectedWorkspaceMountId;
  } else if (state.selectedFolder) {
    body.cwd = state.selectedFolder;
  }
  if (state.workspaceFolders?.length) {
    body.workspaceFolders = state.workspaceFolders;
  }
  if (state.pendingProjectId) {
    body.projectId = state.pendingProjectId;
  }
  if (state.pendingNewSessionThinkingLevel) {
    body.thinkingLevel = state.pendingNewSessionThinkingLevel;
  }
  if (state.pendingNewSessionPermissionMode) {
    body.permissionMode = state.pendingNewSessionPermissionMode;
  }
  if (state.selectedAgentId && state.selectedAgentId !== state.currentAgentId) {
    body.agentId = state.selectedAgentId;
  }
  body.currentSessionPath = state.currentSessionPath;
  return body;
}

function pendingSessionCreateKey(body: PendingSessionCreateBody): string {
  return JSON.stringify(body);
}

function currentPendingSessionDraft(): { body: PendingSessionCreateBody; key: string } | null {
  const state = useStore.getState() as Record<string, any>;
  if (state.pendingNewSession !== true || !normalizeSessionId(state.pendingDraftId)) return null;
  const body = buildPendingSessionCreateBody(state);
  return { body, key: `${state.pendingDraftId}:${pendingSessionCreateKey(body)}` };
}

async function postPendingSessionCreate(body: PendingSessionCreateBody): Promise<any> {
  const res = await lingxiFetch('/api/sessions/new-detached', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
    throwOnHttpError: false,
  });
  return res.json();
}

function stageDetachedSessionForActivation(data: any, ref: Readonly<SessionRef>, state: Record<string, any>): void {
  const existing = sessionByIdentityOrPath(state, ref.sessionId, ref.sessionPath);
  const projection = {
    ...(existing || {}),
    path: ref.sessionPath,
    sessionId: ref.sessionId,
    agentId: ref.agentId,
    agentName: data.agentName || existing?.agentName || ref.agentId,
    cwd: data.cwd || existing?.cwd || null,
    workspaceMountId: data.workspaceMountId || existing?.workspaceMountId || null,
    workspaceLabel: data.workspaceLabel || existing?.workspaceLabel || null,
    title: existing?.title ?? null,
    firstMessage: existing?.firstMessage ?? '',
    modified: existing?.modified || new Date().toISOString(),
    messageCount: existing?.messageCount ?? 0,
    _optimistic: true,
  };
  const sessions = [projection, ...(state.sessions || []).filter((item: any) => (
    normalizeSessionId(item?.sessionId) !== ref.sessionId && item?.path !== ref.sessionPath
  ))];
  const targetKey = ref.sessionId;
  useStore.setState({
    sessions,
    sessionLocatorsById: {
      ...(state.sessionLocatorsById || {}),
      [ref.sessionId]: { path: ref.sessionPath },
    },
    attachedFilesBySession: {
      ...(state.attachedFilesBySession || {}),
      [targetKey]: [...(state.attachedFiles || [])],
    },
  });
  // 不在此处预种空缓存：种了空 items 会让 switchSession 的 hasData 判据为真而跳过
  // 历史加载，新会话首屏从此完全依赖 WS 事件——事件被入口闸门丢弃/迟到即整片空白。
  // 交给 switchSession 的 !hasData 路径走 loadMessages（空历史也会 stamp revision，
  // 供后续 reconcile 补拉自愈）；WS 先到时 session_user_message 侧自会 initSession。
}

export async function loadPendingNewSessionPermissionDefault(): Promise<SessionPermissionMode> {
  try {
    const res = await lingxiFetch('/api/preferences/session-permission-default');
    const data = await res.json();
    const mode = normalizeSessionPermissionMode(data.permissionMode);
    if (isPendingNewSessionDraftView()) emitSessionPermissionMode(mode);
    return mode;
  } catch (err) {
    console.warn('[session] load permission default failed:', err);
    if (isPendingNewSessionDraftView()) emitSessionPermissionMode('ask');
    return 'ask';
  }
}

export async function createNewSession(options: CreateNewSessionOptions = {}): Promise<void> {
  // Entering the pending new-session workspace is a navigation boundary.
  // Any in-flight switchSession response now belongs to the previous view.
  invalidateSessionSwitches();

  // 关闭浮动面板
  if (useStore.getState().activePanel === 'activity') {
    useStore.getState().setActivePanel(null);
  }

  const s = useStore.getState();
  const primaryAgent = findPrimaryAgent(s.agents);
  const primaryWorkspace = resolveAgentWorkspace(primaryAgent);
  const requestedFolder = typeof options.cwd === 'string' && options.cwd.trim() ? options.cwd.trim() : null;
  // 清空会话身份前先冻结当前会话的主工作台。显式 cwd 仍拥有最高优先级；
  // 只继承主工作台，不带额外授权目录、附件或桌面子目录位置。
  const currentProjection = sessionByIdentityOrPath(
    s as Record<string, any>,
    normalizeSessionId(s.currentSessionId),
    typeof s.currentSessionPath === 'string' ? s.currentSessionPath : null,
  );
  const inheritedMountId = requestedFolder ? null : normalizeSessionId(currentProjection?.workspaceMountId);
  // 继承/保留的本地目录统一走 normalizeWorkspacePath（与 applyFolder 落 selectedFolder 的
  // 规范形态一致）：服务端 cwd 在 Windows 上是反斜杠原生路径，直接落 selectedFolder 会让
  // 全部按 '/' 取目录名的显示位退化为整条路径。
  const inheritedLocalFolder = !requestedFolder && !inheritedMountId
    && typeof currentProjection?.cwd === 'string' && currentProjection.cwd.trim()
    ? normalizeWorkspacePath(currentProjection.cwd)
    : null;
  // 无当前会话时继承「当前显示的工作台」：草稿选择（applyFolder/applyStudioWorkspace 写入的
  // selectedFolder / selectedWorkspaceMountId）优先，其次 desk 已激活身份（冷启动恢复窗口）；
  // 两者皆空才落 Primary Agent 工作台（设置页「新建对话默认工作台」语义）。
  // 不做这一层时，用户切到非默认工作台后点新建聊天，草稿会被拽回 Primary 工作台，
  // 左栏列表作用域（resolveWorkspaceScope 草稿态读 selected*）跟着换轨成默认工作台的记录。
  const pendingMountId = !requestedFolder && !inheritedMountId
    ? normalizeSessionId(s.selectedWorkspaceMountId) : null;
  const deskMountId = !requestedFolder && !inheritedMountId && !pendingMountId
    ? normalizeSessionId(s.deskWorkspaceMountId) : null;
  const keptSelectedFolder = !requestedFolder && !inheritedMountId && !pendingMountId && !deskMountId
    && typeof s.selectedFolder === 'string' && s.selectedFolder.trim()
    ? normalizeWorkspacePath(s.selectedFolder)
    : null;
  // deskBasePath 只在本地目录工作台下是真实路径；mount 工作台下它是 'studio:*' 键形式，
  // 不能当文件夹用（mount 情形已由上面的 deskMountId 分支接管）。
  const keptDeskFolder = !requestedFolder && !inheritedMountId && !pendingMountId && !deskMountId && !keptSelectedFolder
    && typeof s.deskBasePath === 'string' && s.deskBasePath.trim()
    ? normalizeWorkspacePath(s.deskBasePath)
    : null;
  const defaultWorkspaceMountId = inheritedMountId || pendingMountId || deskMountId;
  const defaultWorkspaceLabel = inheritedMountId
    ? (typeof currentProjection?.workspaceLabel === 'string' ? currentProjection.workspaceLabel : null)
    : (pendingMountId
      ? (typeof s.selectedWorkspaceLabel === 'string' ? s.selectedWorkspaceLabel : null)
      : (deskMountId ? (typeof s.deskWorkspaceLabel === 'string' ? s.deskWorkspaceLabel : null) : null));
  const defaultFolder = requestedFolder
    || inheritedLocalFolder
    || keptSelectedFolder
    || keptDeskFolder
    // Primary 兜底只在「无会话且未选中任何工作台（含 mount）」时生效，
    // 否则 mount 已选中时 selectedFolder 仍会被写成 Primary 路径污染草稿状态。
    || (!currentProjection && !defaultWorkspaceMountId ? primaryWorkspace || (!primaryAgent ? s.homeFolder : null) : null)
    || null;
  const deskFolder = inheritedMountId && typeof currentProjection?.cwd === 'string'
    ? currentProjection.cwd
    : defaultFolder;
  // 规则 B 补全（2026-09-05 用户拍板）：助手身份与工作台一样跟随「当前」，不再重置回
  // Primary。selectedAgentId=null 即「跟随 currentAgentId」（欢迎页显示与建会话请求体
  // 同语义，与 handleSelectHistory 的 null 约定一致）；仅在没有当前助手时才显式落
  // Primary 兜底。
  const selectedAgentIdForDraft = s.currentAgentId
    ? null
    : (primaryAgent ? primaryAgent.id : null);
  const pendingProjectId = typeof options.projectId === 'string' && options.projectId.trim()
    ? options.projectId.trim()
    : null;

  useStore.setState({
    welcomeVisible: true,
    currentSessionPath: null,
    currentSessionId: null,
    pendingSessionSwitchPath: null,
    // 新建聊天跟随当前：助手与工作台都不重置回 Primary；仅无当前助手且未选中任何
    // 工作台时才落 Primary 兜底（设置页「新建对话默认工作台」语义保留）。
    selectedFolder: defaultFolder,
    selectedWorkspaceMountId: defaultWorkspaceMountId,
    selectedWorkspaceLabel: defaultWorkspaceLabel,
    workspaceFolders: [],
    selectedAgentId: selectedAgentIdForDraft,
    ...pendingNewSessionIdentityPatch(),
    pendingProjectId,
    pendingNewSessionThinkingLevel: null,
    pendingNewSessionPermissionMode: null,
    attachedFiles: [],
    deskContextAttached: false,
    docContextAttached: false,
    deskCurrentPath: '',
    deskFiles: [],
    deskJianContent: null,
  });

  await activateWorkspaceDesk(deskFolder, {
    mountId: defaultWorkspaceMountId,
    label: defaultWorkspaceLabel,
  });

  // 重置 context ring
  useStore.setState({ contextTokens: null, contextWindow: null, contextPercent: null });
  await loadPendingNewSessionPermissionDefault();

  try {
    const res = await lingxiFetch('/api/session-thinking-level?pendingNewSession=1');
    const data = await res.json();
    if (data.thinkingLevel && isPendingNewSessionDraftView()) {
      useStore.getState().setThinkingLevel(data.thinkingLevel);
      useStore.getState().setPendingNewSessionThinkingLevel(data.thinkingLevel);
    }
  } catch {
    useStore.getState().setPendingNewSessionThinkingLevel(null);
  }

  // pending 状态下刷新 model 列表，让 ModelSelector 显示 agent Chat 默认 model
  loadModels();

  requestChatInputFocus(null);
}

// ══════════════════════════════════════════════════════
// 确保 Session 存在（首次发消息时调用）
// ══════════════════════════════════════════════════════

export async function ensureSession(expectedPendingDraftId?: string | null): Promise<Readonly<SessionRef> | null> {
  try {
    const initialState = useStore.getState() as Record<string, any>;
    if (initialState.pendingNewSession !== true) return frozenSessionRefFromState(initialState);
    const draft = currentPendingSessionDraft();
    if (!draft) throw new Error('pending session draft identity is missing');
    const draftId = normalizeSessionId(initialState.pendingDraftId);
    if (expectedPendingDraftId && draftId !== expectedPendingDraftId) return null;

    const data = await postPendingSessionCreate(draft.body);
    // 带上错误码，呈现层才能把它翻成人话；没有码的原生崩溃走兜底文案 + 详情。
    if (data?.error) {
      const routeError = normalizeSessionRouteError(data);
      throw errorWithCode(routeError.message, routeError.code);
    }
    const ref = frozenSessionRefFromCreateResponse(data);
    if (!ref) throw new Error('session creation returned an incomplete session identity');

    const latestDraft = currentPendingSessionDraft();
    const latestState = useStore.getState() as Record<string, any>;
    const stillOwnsPendingView = latestDraft?.key === draft.key
      && normalizeSessionId(latestState.pendingDraftId) === draftId;
    if (!stillOwnsPendingView) return ref;

    stageDetachedSessionForActivation(data, ref, latestState);
    await switchSession(ref.sessionPath);
    const activated = useStore.getState() as Record<string, any>;
    if (activated.currentSessionId === ref.sessionId && activated.currentSessionPath === ref.sessionPath) {
      activated.clearDraft?.(HOME_DRAFT_KEY);
      activated.clearDraft?.(ref.sessionId);
      activated.clearDraft?.(ref.sessionPath);
      useStore.setState({ pendingDraftId: null });
    }
    return ref;
  } catch (err) {
    console.error('[session] create failed:', err);
    showSessionCreationError(err);
    return null;
  }
}

export async function continueDeletedAgentSession(path: string): Promise<boolean> {
  try {
    const res = await lingxiFetch('/api/sessions/continue-deleted-agent', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
    });
    const data = await res.json();
    if (!res.ok || data.error || !data.path) {
      const routeError = normalizeSessionRouteError(data);
      const message = routeError.message || res.statusText || 'continue failed';
      console.error('[session] continue deleted-agent session failed:', message, routeError.code || '');
      // 跟下面 catch 分支同一套呈现：错误码翻成人话，原始英文留在详情，toast 带码。
      const entry = presentErrorWithLabel(
        tr('session.deletedAgent.continueFailed'),
        errorWithCode(message, routeError.code),
      );
      useStore.getState().addToast(entry.text, 'error', 6000, entry.code ? { errorCode: entry.code } : undefined);
      return false;
    }

    await loadSessions();
    await switchSession(data.path);
    if (data.compactionError) {
      useStore.getState().addToast(
        `${tr('session.deletedAgent.continueCompactionFailed')}: ${data.compactionError}`,
        'warning',
        6000,
      );
    }
    return true;
  } catch (err) {
    console.error('[session] continue deleted-agent session failed:', err);
    useStore.getState().addToast(
      presentErrorWithLabel(tr('session.deletedAgent.continueFailed'), err).text,
      'error',
      6000,
    );
    return false;
  }
}

// ══════════════════════════════════════════════════════
// 归档 Session
// ══════════════════════════════════════════════════════

export async function archiveSession(path: string): Promise<void> {
  try {
    const localSessionId = sessionIdForPathFromState(useStore.getState() as Record<string, any>, path);
    const res = await lingxiFetch('/api/sessions/archive', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        path,
        ...(localSessionId ? { sessionId: localSessionId } : {}),
      }),
    });
    const data = await res.json();
    if (data.error) {
      console.error('[session] archive failed:', data.error);
      showSidebarToast(window.t('session.archiveFailed'));
      return;
    }

    const s = useStore.getState();
    const isCurrent = path === s.currentSessionPath;
    const inDraft = !s.currentSessionPath;
    clearSessionRuntimeCaches(path);
    if (isCurrent) {
      clearChatAction();
    }

    // 归档当前会话（或草稿态归档旧会话）→ 回「新建聊天」草稿态：createNewSession
    // 在置空 current* 前读取被归档会话的工作台归属做继承（规则 B：新建跟随当前），
    // 其写入的 pendingNewSession 同时挡住下面 loadSessions 的「首次加载」自动选中。
    // 旧实现是 switchSession(sessions[0])——全局列表第一条不属于当前工作台，会把
    // 整个桌面拽进别的 工作台（用户复测报告 2026-09-05）。
    if (isCurrent || inDraft) {
      await createNewSession();
    }

    await loadSessions();
  } catch (err) {
    console.error('[session] archive failed:', err);
    showSidebarToast(window.t('session.archiveFailed'));
  }
}

// ══════════════════════════════════════════════════════
// 归档管理：列出 / 恢复 / 永久删 / 批量清理
// ══════════════════════════════════════════════════════

export interface ArchivedSession {
  path: string;
  sessionId?: string | null;
  title: string | null;
  firstMessage?: string | null;
  archivedAt: string;
  sizeBytes: number;
  agentId: string;
  agentName: string;
  agentDeleted?: boolean;
  readOnlyReason?: string | null;
  deletedAt?: string | null;
  /** 工作台归属（归档界面按工作台分组的依据）；老服务端可能不带 */
  cwd?: string | null;
  workspaceMountId?: string | null;
  workspaceLabel?: string | null;
}

export type RestoreResult =
  | { status: 'ok'; restoredPath: string | null; sessionId: string | null }
  | { status: 'conflict'; error?: string }
  | { status: 'error'; error?: string };

export async function listArchivedSessions(): Promise<ArchivedSession[]> {
  try {
    const res = await lingxiFetch('/api/sessions/archived');
    if (!res.ok) return [];
    return await res.json();
  } catch (err) {
    console.error('[archived] list failed:', err);
    return [];
  }
}

export async function restoreSession(
  target: string | Pick<ArchivedSession, 'path' | 'sessionId'>,
  opts?: { /** 批量恢复传 false：不跳转会话，仅恢复进列表（单条恢复默认跳转，行为不变） */
    switchTo?: boolean },
): Promise<RestoreResult> {
  const sessionPath = typeof target === 'string' ? target : target.path;
  const sessionId = typeof target === 'string' ? null : normalizeSessionId(target.sessionId);
  try {
    const res = await lingxiFetch('/api/sessions/restore', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        path: sessionPath,
        ...(sessionId ? { sessionId } : {}),
      }),
    });
    const data = await res.json().catch(() => ({}));
    if (res.status === 409) return { status: 'conflict', error: data?.error };
    if (!res.ok) return { status: 'error', error: data?.error || res.statusText };
    const restoredPath = typeof data?.restoredPath === 'string' ? data.restoredPath : null;
    const restoredSessionId = normalizeSessionId(data?.sessionId) || sessionId;

    await loadSessions();
    const restoredSession = sessionByIdentityOrPath(
      useStore.getState() as Record<string, any>,
      restoredSessionId,
      restoredPath,
    );
    if (opts?.switchTo !== false && restoredSession?.path) {
      await switchSession(restoredSession.path);
    }
    void hydrateInputDrafts();
    return { status: 'ok', restoredPath, sessionId: restoredSessionId };
  } catch (err) {
    console.error('[archived] restore failed:', err);
    return { status: 'error', error: errorMessage(err) };
  }
}

export async function deleteArchivedSession(target: string | Pick<ArchivedSession, 'path' | 'sessionId'>): Promise<boolean> {
  const sessionPath = typeof target === 'string' ? target : target.path;
  const sessionId = typeof target === 'string' ? null : normalizeSessionId(target.sessionId);
  try {
    const res = await lingxiFetch('/api/sessions/archived/delete', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        path: sessionPath,
        ...(sessionId ? { sessionId } : {}),
      }),
    });
    return res.ok;
  } catch (err) {
    console.error('[archived] delete failed:', err);
    return false;
  }
}

export async function cleanupArchivedSessions(maxAgeDays: 30 | 90): Promise<{ deleted: number }> {
  try {
    const res = await lingxiFetch('/api/sessions/cleanup', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ maxAgeDays }),
    });
    if (!res.ok) return { deleted: 0 };
    const data = await res.json();
    return { deleted: data.deleted ?? 0 };
  } catch (err) {
    console.error('[archived] cleanup failed:', err);
    return { deleted: 0 };
  }
}

// ══════════════════════════════════════════════════════
// 工作台处置（移除工作台前二选一：归档 / 永久删除）
// ══════════════════════════════════════════════════════

export interface WorkspaceDisposalResult {
  ok: boolean;
  action: 'archive' | 'delete';
  matched: number;
  disposed: number;
  skippedStreaming: number;
}

export async function disposeWorkspaceSessions(
  identity: { workspaceMountId?: string | null; cwd?: string | null },
  action: 'archive' | 'delete',
): Promise<WorkspaceDisposalResult | null> {
  try {
    const res = await lingxiFetch('/api/sessions/workspace-disposal', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        ...(identity.workspaceMountId ? { workspaceMountId: identity.workspaceMountId } : {}),
        ...(identity.cwd ? { cwd: identity.cwd } : {}),
        action,
      }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok || data?.error) {
      console.error('[workspace-disposal] failed:', data?.error || res.statusText);
      return null;
    }
    await loadSessions();
    return {
      ok: true,
      action,
      matched: Number(data?.matched ?? 0),
      disposed: Number(data?.disposed ?? 0),
      skippedStreaming: Number(data?.skippedStreaming ?? 0),
    };
  } catch (err) {
    console.error('[workspace-disposal] failed:', err);
    return null;
  }
}

/**
 * 孤儿会话清扫（静默自动归档）：所属工作台已被移除（mount 失效）或磁盘目录
 * 已被直接删除的会话，启动时归档进归档界面——那里按工作台分组可见、可整组处理。
 * 用户裁决：不弹任何框。
 */
export async function sweepOrphanedWorkspaceSessions(): Promise<number> {
  try {
    const res = await lingxiFetch('/api/sessions/sweep-orphaned-workspaces', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok || data?.error) {
      console.error('[workspace-sweep] failed:', data?.error || res.statusText);
      return 0;
    }
    const archived = Number(data?.archived ?? 0);
    if (archived > 0) await loadSessions();
    return archived;
  } catch (err) {
    console.error('[workspace-sweep] failed:', err);
    return 0;
  }
}

/** 重新添加工作台时的恢复提示：该路径名下有多少条已归档记录。 */
export async function countArchivedSessionsForWorkspace(identity: {
  workspaceMountId?: string | null;
  cwd?: string | null;
}): Promise<number> {
  const list = await listArchivedSessions();
  const normalizedCwd = identity.cwd ? identity.cwd.replace(/\/+$/, '') : null;
  return list.filter((item) => {
    if (identity.workspaceMountId && item.workspaceMountId === identity.workspaceMountId) return true;
    if (normalizedCwd && item.cwd && item.cwd.replace(/\/+$/, '') === normalizedCwd) return true;
    return false;
  }).length;
}

// ══════════════════════════════════════════════════════
// 重命名 Session
// ══════════════════════════════════════════════════════

export async function renameSession(path: string, title: string): Promise<boolean> {
  try {
    const res = await lingxiFetch('/api/sessions/rename', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path, title }),
    });
    const data = await res.json();
    if (data.error) {
      console.error('[session] rename failed:', data.error);
      return false;
    }
    // 乐观更新 store 中的 title
    const sessions = useStore.getState().sessions.map(s =>
      s.path === path ? { ...s, title } : s,
    );
    useStore.setState({ sessions });
    return true;
  } catch (err) {
    console.error('[session] rename failed:', err);
    return false;
  }
}

// ══════════════════════════════════════════════════════
// 置顶 / 取消置顶 Session
// ══════════════════════════════════════════════════════

export async function pinSession(path: string, pinned: boolean): Promise<boolean> {
  try {
    const localSessionId = sessionIdForPathFromState(useStore.getState() as Record<string, any>, path);
    const res = await lingxiFetch('/api/sessions/pin', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        path,
        ...(localSessionId ? { sessionId: localSessionId } : {}),
        pinned,
      }),
    });
    const data = await res.json();
    if (!res.ok || data.error) {
      console.error('[session] pin failed:', data.error || res.statusText);
      showSidebarToast(window.t(pinned ? 'session.pinFailed' : 'session.unpinFailed'));
      return false;
    }

    const pinnedAt = typeof data.pinnedAt === 'string' ? data.pinnedAt : null;
    const responseSessionId = normalizeSessionId(data.sessionId) || localSessionId;
    const sessions = useStore.getState().sessions.map(s =>
      (responseSessionId && normalizeSessionId(s.sessionId) === responseSessionId) || s.path === path
        ? { ...s, pinnedAt }
        : s,
    );
    useStore.setState({ sessions });
    return true;
  } catch (err) {
    console.error('[session] pin failed:', err);
    showSidebarToast(window.t(pinned ? 'session.pinFailed' : 'session.unpinFailed'));
    return false;
  }
}

/**
 * 提交置顶区的完整新顺序。先按新顺序乐观改写本地 pinOrder（步长与服务端一致），
 * 拖完立刻定位；服务端拒绝或请求失败就整体回滚到提交前的快照并提示，
 * 不留下半套顺序。
 */
export async function reorderPinnedSessions(orderedSessionIds: string[]): Promise<boolean> {
  const sessionIds = Array.isArray(orderedSessionIds)
    ? orderedSessionIds.filter((id): id is string => typeof id === 'string' && !!id.trim())
    : [];
  if (sessionIds.length === 0) return false;

  const snapshot = useStore.getState().sessions;
  const orderById = new Map(sessionIds.map((sessionId, index) => [sessionId, (index + 1) * 1024]));
  useStore.setState({
    sessions: snapshot.map(s => {
      const sessionId = normalizeSessionId(s.sessionId);
      const pinOrder = sessionId ? orderById.get(sessionId) : undefined;
      return pinOrder === undefined ? s : { ...s, pinOrder };
    }),
  });

  try {
    const res = await lingxiFetch('/api/sessions/pin-order', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ sessionIds }),
    });
    const data = await res.json();
    if (!res.ok || data.error) {
      throw new Error(data.error || res.statusText);
    }
    return true;
  } catch (err) {
    console.error('[session] pin reorder failed:', err);
    useStore.setState({ sessions: snapshot });
    showSidebarToast(window.t('session.reorderFailed'));
    return false;
  }
}

// ══════════════════════════════════════════════════════
// 显式更新会话能力（fresh compact）
// ══════════════════════════════════════════════════════

/**
 * 显式刷新 Agent 工具：fresh compact——旧对话压缩成摘要 checkpoint，
 * 用当前配置重建 prompt/工具快照。成功后重新拉取消息（jsonl 多了 compact 记录）。
 */
export async function refreshSessionCapabilities(path: string): Promise<boolean> {
  const store = useStore.getState();
  if (sessionScopedListIncludes(store as Record<string, any>, store.capabilityRefreshingSessions, path)) return false;
  store.setSessionCapabilityRefreshing(path, true);
  try {
    const res = await lingxiFetch('/api/sessions/fresh-compact', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
      // Fresh compact runs an LLM summarization over the whole conversation;
      // long sessions routinely exceed the 30s lingxiFetch default. A premature
      // abort here surfaces a false failure while the server keeps compacting.
      timeout: 180_000,
    });
    const data = await res.json();
    if (!res.ok || data.error) {
      const routeError = normalizeSessionRouteError(data);
      throw errorWithCode(routeError.message || res.statusText, routeError.code);
    }
    await loadMessages(path);
    return true;
  } catch (err) {
    console.error('[session] capability refresh failed:', err);
    const state = useStore.getState();
    state.setInlineError?.(path, presentErrorWithLabel(tr('input.refreshAndCompactFailed'), err), 6000);
    return false;
  } finally {
    useStore.getState().setSessionCapabilityRefreshing(path, false);
  }
}

// ══════════════════════════════════════════════════════
// Toast
// ══════════════════════════════════════════════════════

export function showSidebarToast(text: string, duration = 3000): void {
  useStore.getState().addToast(text, 'info', duration);
}

function tr(key: string): string {
  return typeof window !== 'undefined' && typeof window.t === 'function'
    ? window.t(key)
    : key;
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err || 'Unknown error');
}

/** 内联错误说人话、原始报错留在展开区；toast 一闪而过，只带正文和错误码。 */
function showSessionActionError(labelKey: string, path: string, detail: unknown): void {
  const entry = presentErrorWithLabel(tr(labelKey), detail);
  const state = useStore.getState();
  state.setInlineError?.(path, entry, 6000);
  state.addToast(entry.text, 'error', 6000, entry.code ? { errorCode: entry.code } : undefined);
}

function showSessionCreationError(detail: unknown): void {
  showSessionActionError('session.createFailed', useStore.getState().currentSessionPath || '', detail);
}

function showSessionSwitchError(targetPath: string, detail: unknown): void {
  const state = useStore.getState();
  showSessionActionError('session.switchFailed', state.currentSessionPath || targetPath || '', detail);
}
