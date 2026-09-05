import type { Session } from '../types';
import { isSameWorkspacePath } from '../utils/agent-workspace';

export type DateGroup = 'today' | 'thisWeek' | 'earlier';

export type SessionSection =
  | {
      id: 'pinned';
      kind: 'pinned';
      titleKey: 'sidebar.pinned';
      items: Session[];
    }
  | {
      id: `date:${DateGroup}`;
      kind: 'date';
      titleKey: `time.${DateGroup}`;
      group: DateGroup;
      items: Session[];
    };

interface BuildSessionSectionsOptions {
  mode?: 'time';
  now?: Date;
}

const DATE_GROUP_ORDER: DateGroup[] = ['today', 'thisWeek', 'earlier'];

function getSessionDateGroup(isoStr: string | null, now: Date): DateGroup {
  if (!isoStr) return 'earlier';
  const date = new Date(isoStr);
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const weekAgo = new Date(today);
  weekAgo.setDate(today.getDate() - 7);

  if (date >= today) return 'today';
  if (date >= weekAgo) return 'thisWeek';
  return 'earlier';
}

function isPinnedSession(session: Session): boolean {
  return typeof session.pinnedAt === 'string' && session.pinnedAt.length > 0;
}

function modifiedTime(session: Session): number {
  return timestamp(session.modified);
}

function timestamp(value: string | null | undefined): number {
  if (!value) return 0;
  const time = Date.parse(value);
  return Number.isFinite(time) ? time : 0;
}

function compareByPath(a: Session, b: Session): number {
  return String(a.path || '').localeCompare(String(b.path || ''));
}

function compareByModifiedDesc(a: Session, b: Session): number {
  return modifiedTime(b) - modifiedTime(a) || compareByPath(a, b);
}

/**
 * 置顶区按手动顺序升序。还没有顺序的会话排在有顺序的之后，彼此之间仍按最近活动排——
 * 顺序固化前的老数据因此保持原来的显示顺序。
 */
function comparePinned(a: Session, b: Session): number {
  const ao = typeof a.pinOrder === 'number' ? a.pinOrder : Number.POSITIVE_INFINITY;
  const bo = typeof b.pinOrder === 'number' ? b.pinOrder : Number.POSITIVE_INFINITY;
  if (ao !== bo) return ao - bo;
  return compareByModifiedDesc(a, b);
}

export function buildSessionSections(
  sessions: Session[],
  options: BuildSessionSectionsOptions = {},
): SessionSection[] {
  const pinned = sessions
    .filter(isPinnedSession)
    .sort(comparePinned);
  const regular = sessions.filter(session => !isPinnedSession(session));

  const sections: SessionSection[] = [];
  sections.push({
    id: 'pinned',
    kind: 'pinned',
    titleKey: 'sidebar.pinned',
    items: pinned,
  });

  const now = options.now ?? new Date();
  const dateGroups: Record<DateGroup, Session[]> = {
    today: [],
    thisWeek: [],
    earlier: [],
  };
  for (const session of regular) {
    dateGroups[getSessionDateGroup(session.modified, now)].push(session);
  }

  // Sort within each group: newest modified first
  for (const group of DATE_GROUP_ORDER) {
    dateGroups[group].sort(compareByModifiedDesc);
  }

  for (const group of DATE_GROUP_ORDER) {
    const items = dateGroups[group];
    if (items.length === 0) continue;
    sections.push({
      id: `date:${group}`,
      kind: 'date',
      titleKey: `time.${group}`,
      group,
      items,
    });
  }

  return sections;
}

// ── Workspace 作用域（任务七/八：左栏聊天列表只显示当前工作台的会话） ──

/** 当前工作台身份：mount 工作台用 mountId，本地目录工作台用规范化后的 basePath。 */
export interface WorkspaceScope {
  mountId: string | null;
  basePath: string | null;
  /**
   * 默认工作台双形态合流键：默认工作台（mount "default"）的本地根路径。
   * 默认工作台与「Agent 工作台目录」是同一目录的两个入口——mount 形态（经工作台
   * 切换器/挂载创建）与本地路径形态（历史 cwd、旧版本创建）的会话同属一个工作台。
   * 已知根路径时两个方向的匹配都放行；其余 mount 保持严格互斥。
   */
  defaultRootPath?: string | null;
}

function normalizeScopeMountId(value: string | null | undefined): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null;
}

function withDualDefaultRoot(scope: WorkspaceScope, defaultRootPath: string | null): WorkspaceScope {
  // 仅在合流键已知时附加，避免无谓改变既有 scope 形状。
  return defaultRootPath ? { ...scope, defaultRootPath } : scope;
}

function dualDefaultRootForPath(candidatePath: string | null | undefined, defaultRootPath: string | null): string | null {
  if (!defaultRootPath) return null;
  return isSameWorkspacePath(candidatePath, defaultRootPath) ? defaultRootPath : null;
}

/**
 * 解析当前左栏应使用的工作台身份。
 *
 * - 有当前会话时以 desk（已激活工作台）为准：mount 优先，否则 deskBasePath；
 * - pending 新会话（无 currentSessionPath）时以 pending 目标（selectedWorkspaceMountId /
 *   selectedFolder）为准；两者都未落地时退回 desk 身份（启动窗口期 desk 先恢复）；
 * - defaultWorkspaceRootPath 已知时：作用域落在默认工作台（mount "default" 或其本地
 *   路径）会携带合流键 defaultRootPath，供 sessionBelongsToWorkspaceScope 双形态匹配。
 */
export function resolveWorkspaceScope(state: {
  currentSessionPath: string | null;
  deskWorkspaceMountId: string | null;
  deskBasePath: string | null;
  selectedWorkspaceMountId: string | null;
  selectedFolder: string | null;
  defaultWorkspaceRootPath?: string | null;
}): WorkspaceScope {
  const defaultRootPath = typeof state.defaultWorkspaceRootPath === 'string'
    ? state.defaultWorkspaceRootPath
    : null;
  if (!state.currentSessionPath) {
    const pendingMountId = normalizeScopeMountId(state.selectedWorkspaceMountId);
    if (pendingMountId) {
      return withDualDefaultRoot(
        { mountId: pendingMountId, basePath: null },
        pendingMountId === 'default' ? defaultRootPath : null,
      );
    }
    if (state.selectedFolder) {
      return withDualDefaultRoot(
        { mountId: null, basePath: state.selectedFolder },
        dualDefaultRootForPath(state.selectedFolder, defaultRootPath),
      );
    }
  }
  const deskMountId = normalizeScopeMountId(state.deskWorkspaceMountId);
  if (deskMountId) {
    return withDualDefaultRoot(
      { mountId: deskMountId, basePath: null },
      deskMountId === 'default' ? defaultRootPath : null,
    );
  }
  const deskBasePath = state.deskBasePath || null;
  return withDualDefaultRoot(
    { mountId: null, basePath: deskBasePath },
    dualDefaultRootForPath(deskBasePath, defaultRootPath),
  );
}

/**
 * Session 是否属于给定工作台作用域（数据层判定，非视觉过滤）：
 *
 * - mount 作用域：session.workspaceMountId 严格等值（不做显示名模糊匹配）；
 * - 本地目录作用域：带 workspaceMountId 的 session 不混入；其余按项目现有
 *   规范化路径规则（isSameWorkspacePath：反斜杠/尾斜杠归一、Windows/UNC 大小写不敏感）
 *   比较 session.cwd 与作用域根；cwd 缺失视为无身份，不归属。
 * - 默认工作台例外（双形态合流）：scope 携带 defaultRootPath 时——
 *   mount "default" 作用域同时收 cwd 指向该根路径的旧形态会话；
 *   该根路径的本地作用域同时收 mount "default" 会话。其余 mount 不合流。
 */
export function sessionBelongsToWorkspaceScope(
  session: Pick<Session, 'cwd' | 'workspaceMountId'>,
  scope: WorkspaceScope,
): boolean {
  const scopeMountId = normalizeScopeMountId(scope.mountId);
  const sessionMountId = normalizeScopeMountId(session.workspaceMountId);
  if (scopeMountId) {
    if (sessionMountId) return sessionMountId === scopeMountId;
    if (scopeMountId === 'default' && scope.defaultRootPath) {
      return isSameWorkspacePath(session.cwd, scope.defaultRootPath);
    }
    return false;
  }
  if (sessionMountId) {
    return sessionMountId === 'default'
      && !!scope.defaultRootPath
      && !!scope.basePath
      && isSameWorkspacePath(scope.basePath, scope.defaultRootPath);
  }
  if (!scope.basePath) return false;
  return isSameWorkspacePath(session.cwd, scope.basePath);
}

export function filterSessionsForWorkspaceScope(
  sessions: Session[],
  scope: WorkspaceScope,
): Session[] {
  return sessions.filter(session => sessionBelongsToWorkspaceScope(session, scope));
}
