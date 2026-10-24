/**
 * conversation-map-projection — 会话地图数据投影层
 *
 * 纯函数部分（buildMapWorkspaces / applyTurnsToThreads）不触碰 store 与网络，
 * 便于单测；fetch/load 部分通过 lingxiFetch + useStore 读写 turns 缓存。
 */

import type { Session } from '../../types';
import { useStore } from '../../stores';
import type { MapPendingTurn, MapTurn, MapTurnsEntry } from '../../stores/conversation-map-slice';
import { lingxiFetch } from '../../services/service-client';
import { resolveServerConnection } from '../../services/service-client';
import { captureSessionConnection, ownsSessionConnection, sessionOwnerFetch } from '../../services/session-connection-owner';
import { sessionIdForPathFromLocatorState } from '../../stores/session-slice';
import {
  autoProjectIdForCwd,
  cwdFromAutoProjectId,
} from '../../../../../shared/session-projects.ts';

export interface MapThread {
  sessionId: string;
  sessionPath: string;
  title: string;
  agentId: string | null;
  agentName: string | null;
  cwd: string | null;
  modified: string | null;
  revision?: string | null;
  forkedFrom: { sessionId: string; entryId?: string } | null;
  color: string;
  turns: MapTurn[];
  hiddenTurnCount: number;
}

export interface MapWorkspace {
  key: string;
  title: string;
  threads: MapThread[];
}

export const MAP_THREAD_COLORS = ['#0f766e', '#2563eb', '#be123c', '#7c3aed', '#b45309'];

const UNGROUPED_KEY = 'map-ungrouped';

type TranslateFn = (key: string) => string;

function lastPathSegment(cwd: string): string {
  const trimmed = cwd.replace(/[/\\]+$/g, '');
  const parts = trimmed.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] || cwd;
}

function workspaceTitleForKey(
  key: string,
  projects: { id: string; name: string }[],
  t?: TranslateFn,
): string {
  const project = projects.find((p) => p.id === key);
  if (project) return project.name;
  const cwd = cwdFromAutoProjectId(key);
  if (cwd) return lastPathSegment(cwd);
  return t?.('map.ungrouped') ?? 'Ungrouped';
}

export function buildMapWorkspaces(
  sessions: Session[],
  projects: { id: string; name: string }[],
  t?: TranslateFn,
): MapWorkspace[] {
  const groups = new Map<string, Session[]>();
  for (const session of sessions) {
    if (!session || ((session.messageCount ?? 0) <= 0 && (session as Session & {hasDisplayMessages?: boolean}).hasDisplayMessages !== true)) continue;
    const key = session.projectId
      || (session.cwd ? autoProjectIdForCwd(session.cwd) : UNGROUPED_KEY);
    const list = groups.get(key);
    if (list) list.push(session);
    else groups.set(key, [session]);
  }

  const workspaces: MapWorkspace[] = [];
  for (const [key, groupSessions] of groups) {
    const threads: MapThread[] = groupSessions
      .slice()
      .sort((a, b) => (a.modified || '').localeCompare(b.modified || ''))
      .map((session, index) => ({
        sessionId: session.sessionId || session.path,
        sessionPath: session.path,
        title: session.title || session.firstMessage || session.path,
        agentId: session.agentId ?? null,
        agentName: session.agentName ?? null,
        cwd: session.cwd ?? null,
        modified: session.modified || null,
        revision: (session as typeof session & {revision?: string}).revision ?? session.modified ?? null,
        forkedFrom: session.forkedFrom
          ? { sessionId: session.forkedFrom.sessionId, ...(session.forkedFrom.entryId ? { entryId: session.forkedFrom.entryId } : {}) }
          : null,
        color: MAP_THREAD_COLORS[index % MAP_THREAD_COLORS.length],
        turns: [],
        hiddenTurnCount: 0,
      }));
    workspaces.push({
      key,
      title: workspaceTitleForKey(key, projects, t),
      threads,
    });
  }

  const mostRecentModified = (ws: MapWorkspace): string => {
    let latest = '';
    for (const thread of ws.threads) {
      if (thread.modified && thread.modified > latest) latest = thread.modified;
    }
    return latest;
  };
  workspaces.sort((a, b) => mostRecentModified(b).localeCompare(mostRecentModified(a)));
  return workspaces;
}

/**
 * 把已加载的 turns 附加到各 thread，并按 fork 边界过滤：
 * fork 会话只显示分叉点之后的轮次，之前的记为 hiddenTurnCount。
 */
export function applyTurnsToThreads(
  workspaces: MapWorkspace[],
  turnsBySessionId: Record<string, MapTurnsEntry>,
): MapWorkspace[] {
  return workspaces.map((workspace) => ({
    ...workspace,
    threads: workspace.threads.map((thread) => {
      const entry = turnsBySessionId[thread.sessionId];
      if (!entry || entry.loading || entry.error) {
        return { ...thread, turns: [], hiddenTurnCount: 0 };
      }
      const turns = entry.turns;
      if (!thread.forkedFrom) {
        return { ...thread, turns, hiddenTurnCount: 0 };
      }
      // entryId 可缺（引用型子对话）：缺时不裁剪（挂全量轮次）。
      const forkIndex = thread.forkedFrom.entryId
        ? turns.findIndex((turn) =>
          turn.entryIds.includes(thread.forkedFrom!.entryId!))
        : -1;
      if (forkIndex < 0) {
        return { ...thread, turns, hiddenTurnCount: 0 };
      }
      return {
        ...thread,
        turns: turns.slice(forkIndex + 1),
        hiddenTurnCount: forkIndex + 1,
      };
    }),
  }));
}

/** 待回复卡片的安全网寿命：超过即清除，避免悬挂。 */
export const MAP_PENDING_TURN_MAX_AGE_MS = 15 * 60 * 1000;

/**
 * 判定某会话的待回复条目是否可以清除：
 * 问题已落盘且（答复非空 或 会话已不在流式输出）→ 交接给正式卡片；
 * 超时条目一律清除。
 */
export function shouldClearMapPendingTurn(
  turns: MapTurn[],
  pending: MapPendingTurn,
  isStreaming: boolean,
): boolean {
  if (Date.now() - pending.startedAt > MAP_PENDING_TURN_MAX_AGE_MS) return true;
  const committed = turns.find((turn) => turn.question.trim() === pending.question.trim());
  if (!committed) return false;
  return committed.answer.trim().length > 0 || !isStreaming;
}

function mapOwner() {
  return resolveServerConnection(useStore.getState())?.serverNodeKind === 'lingxi-service'
    ? captureSessionConnection() : null;
}
function validTurns(value: unknown): value is MapTurn[] {
  return Array.isArray(value) && value.every(turn => turn && typeof turn === 'object'
    && Number.isSafeInteger(turn.turnIndex) && turn.turnIndex >= 0
    && typeof turn.question === 'string' && typeof turn.answer === 'string'
    && [turn.questionEntryId, turn.answerEntryId].every(id => id === null || typeof id === 'string')
    && Array.isArray(turn.entryIds) && turn.entryIds.every((id: unknown) => typeof id === 'string')
    && Number.isSafeInteger(turn.processCount) && turn.processCount >= 0 && typeof turn.truncated === 'boolean');
}

export async function fetchMapTurns(  session: { sessionId?: string | null; path: string },
): Promise<MapTurn[]> {
  const owner = mapOwner();
  if (owner) {
    const id = session.sessionId || sessionIdForPathFromLocatorState(useStore.getState(), session.path);
    if (!id) throw new Error('地图会话身份尚未就绪');
    const response = await sessionOwnerFetch(owner, `/lingxi/v1/sessions/${encodeURIComponent(id)}/conversation-map/turns`);
    const body = await response.json() as {sessionId?: unknown; turns?: unknown};
    if (!ownsSessionConnection(owner)) throw new Error('连接已切换，已忽略旧地图');
    if (body.sessionId !== id || !validTurns(body.turns)) throw new Error('地图回合格式无效');
    return body.turns;
  }
  const query = session.sessionId
    ? `sessionId=${encodeURIComponent(session.sessionId)}`
    : `path=${encodeURIComponent(session.path)}`;
  const res = await lingxiFetch(`/api/conversation-map/turns?${query}`);
  const body = await res.json() as { turns?: MapTurn[] };
  return Array.isArray(body.turns) ? body.turns : [];
}

export interface MapLayoutPayload {
  positions?: Record<string, { x: number; y: number }>;
  collapsed?: string[];
  replacePositions?: boolean;
}

export async function fetchMapLayout(): Promise<{
  positions: Record<string, { x: number; y: number }>;
  collapsed: string[];
}> {
  const owner = mapOwner();
  const res = owner ? await sessionOwnerFetch(owner, '/lingxi/v1/conversation-map/layout') : await lingxiFetch('/api/conversation-map/layout');
  const body = await res.json() as { version?: unknown; positions?: unknown; collapsed?: unknown; degraded?: unknown; readError?: {message?: unknown} };
  if (owner && (!ownsSessionConnection(owner) || body.version !== 1 || !body.positions || typeof body.positions !== 'object' || Array.isArray(body.positions) || !Array.isArray(body.collapsed))) throw new Error('地图布局读取失败或连接已切换');
  if (owner && body.degraded === true) throw new Error(typeof body.readError?.message === 'string' ? body.readError.message : '地图布局已降级，保留当前内容');
  const positions: Record<string, { x: number; y: number }> = {};
  if (body.positions && typeof body.positions === 'object' && !Array.isArray(body.positions)) {
    for (const [key, value] of Object.entries(body.positions as Record<string, unknown>)) {
      const pos = value as { x?: unknown; y?: unknown } | null;
      if (pos && typeof pos.x === 'number' && typeof pos.y === 'number'
        && Number.isFinite(pos.x) && Number.isFinite(pos.y)) {
        positions[key] = { x: pos.x, y: pos.y };
      }
    }
  }
  const collapsed = Array.isArray(body.collapsed)
    ? (body.collapsed as unknown[]).filter((item): item is string => typeof item === 'string')
    : [];
  return { positions, collapsed };
}

/**
 * 视觉偏好持久化：fire-and-forget。失败只 console.warn，
 * 布局写盘绝不能破坏地图交互。
 */
export function persistMapLayout(patch: MapLayoutPayload): void {
  const owner = mapOwner();
  if (owner) {
    // 非有限坐标不能经 JSON 变成 null 后污染真实布局。
    if (patch.positions && Object.values(patch.positions).some(pos => !pos || !Number.isFinite(pos.x) || !Number.isFinite(pos.y))) {
      console.warn('[conversation-map] persist layout failed', new Error('地图坐标无效')); return;
    }
    void sessionOwnerFetch(owner, '/lingxi/v1/conversation-map/layout', {method:'PUT', headers:{'content-type':'application/json'}, body:JSON.stringify(patch)})
      .catch(error => console.warn('[conversation-map] persist layout failed', error));
    return;
  }
  void lingxiFetch('/api/conversation-map/layout', {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(patch),
  }).catch((err) => {
    console.warn('[conversation-map] persist layout failed', err);
  });
}

const TURN_FETCH_CONCURRENCY = 4;

/**
 * 为一个工作区内的所有 thread 加载 turns（跳过已有新鲜缓存的）。
 * 以权威 revision 作为缓存令牌（旧服务沿 modified）；并发上限 4（简单 worker-pool）。
 */
export async function loadTurnsForWorkspace(workspace: MapWorkspace): Promise<void> {
  const owner = mapOwner();
  const store = useStore.getState();
  const stale = workspace.threads.filter((thread) => {
    const entry = store.mapTurnsBySessionId[thread.sessionId];
    if (entry?.loading) return false;
    return !(entry && entry.revision === (thread.revision ?? thread.modified));
  });
  if (stale.length === 0) return;

  for (const thread of stale) {
    useStore.getState().setMapTurnsEntry(thread.sessionId, {
      revision: null,
      turns: [],
      loading: true,
      error: null,
    });
  }

  let cursor = 0;
  const worker = async (): Promise<void> => {
    while (cursor < stale.length) {
      if (owner && !ownsSessionConnection(owner)) return;
      const thread = stale[cursor++];
      try {
        const turns = await fetchMapTurns({
          sessionId: thread.sessionId === thread.sessionPath ? null : thread.sessionId,
          path: thread.sessionPath,
        });
        if (owner && !ownsSessionConnection(owner)) return;
        useStore.getState().setMapTurnsEntry(thread.sessionId, {
          revision: thread.revision ?? thread.modified,
          turns,
          loading: false,
          error: null,
        });
      } catch (err) {
        if (owner && !ownsSessionConnection(owner)) return;
        useStore.getState().setMapTurnsEntry(thread.sessionId, {
          revision: null,
          turns: [],
          loading: false,
          error: err instanceof Error ? err.message : String(err),
        });
      }
    }
  };
  const workers = Array.from(
    { length: Math.min(TURN_FETCH_CONCURRENCY, stale.length) },
    () => worker(),
  );
  await Promise.all(workers);
}
