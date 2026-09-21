import type { Session } from '../types';
import { isSameWorkspacePath } from '../utils/agent-workspace';
import { normalizeWorkspacePath } from '../../../../shared/workspace-history.ts';

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
  /** false 时跳过置顶区（项目组内部分组用；置顶区在列表顶部全局渲染一次）。 */
  includePinned?: boolean;
}

export interface SessionChildFoldOptions {
  /** 被折叠的主对话 sessionId 集合（点击主对话收起子对话） */
  foldedParentIds?: ReadonlySet<string>;
  /** 被折叠也必须展示的会话路径（当前正在聊的子对话） */
  alwaysShowPaths?: ReadonlySet<string>;
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

/** 置顶判断供列表分组复用：置顶会话不参与项目/执行中分组，始终留在顶部置顶区。 */
export function isPinnedSession(session: Session): boolean {
  return typeof session.pinnedAt === 'string' && session.pinnedAt.length > 0;
}

export interface ChildSessionSplit {
  topLevel: Session[];
  childrenByParentId: Map<string, Session[]>;
}

/**
 * 拆分子对话（谱系分组）：forkedFrom 指向列表内另一会话且自身未置顶 → 子行；
 * 其余全部顶层（含孤儿：主对话已归档/不在当前列表/被置顶）。子行同级按最近
 * 活动排序。只拆分不改写输入对象；子行在 expandChildRows 里克隆时才打标注。
 */
export function splitChildSessions(sessions: Session[]): ChildSessionSplit {
  const present = new Set<string>();
  for (const session of sessions) {
    if (session.sessionId) present.add(session.sessionId);
  }
  // 先建父级索引并识别谱系环：环上的会话沿父级永远走不到根，按顶层处理，
  // 否则它们会互相挂载、谁都不在顶层序列里，从列表里双双消失。
  const parentOf = new Map<string, string>();
  for (const session of sessions) {
    const parentId = session.forkedFrom?.sessionId || null;
    if (parentId && session.sessionId && session.sessionId !== parentId && present.has(parentId)) {
      parentOf.set(session.sessionId, parentId);
    }
  }
  const cyclic = new Set<string>();
  for (const sessionId of parentOf.keys()) {
    const seen = new Set<string>([sessionId]);
    let current = sessionId;
    while (true) {
      const parent = parentOf.get(current);
      if (!parent) break;
      if (seen.has(parent)) {
        for (const id of seen) cyclic.add(id);
        break;
      }
      seen.add(parent);
      current = parent;
    }
  }
  const topLevel: Session[] = [];
  const childrenByParentId = new Map<string, Session[]>();
  for (const session of sessions) {
    const parentId = session.forkedFrom?.sessionId || null;
    if (
      parentId
      && session.sessionId
      && session.sessionId !== parentId
      && present.has(parentId)
      && !cyclic.has(session.sessionId)
      && !isPinnedSession(session)
    ) {
      const list = childrenByParentId.get(parentId) || [];
      list.push(session);
      childrenByParentId.set(parentId, list);
      continue;
    }
    topLevel.push(session);
  }
  for (const list of childrenByParentId.values()) list.sort(compareByModifiedDesc);
  return { topLevel, childrenByParentId };
}

/**
 * 谱系树展示上限：主对话 + 两层派生（子、孙），共三层。与创建侧的 fork 深度上限
 * （server sessions 路由 MAX_FORK_LINEAGE_DEPTH）同口径；超出两层的只可能是
 * 历史遗留数据，展示时收拢到第一层祖先名下，保证「数据在列表里就能看见」。
 */
const MAX_LINEAGE_DEPTH = 2;

/**
 * 顶层序列展开子行：子对话紧跟其主对话之后，克隆时写入 childOfSessionId
 * 展示提示（行渲染据此缩进）；带子对话的行克隆时写入 hasChildSessions 提示
 * （行渲染据此展示折叠开关），第二层子行再带 childDepth 供行渲染加深缩进。
 * options.foldedParentIds：被折叠的父对话 sessionId 集合，其子对话不展开；
 * options.alwaysShowPaths：即使被折叠也必须展示的行（如当前正在聊的会话），
 * 防止“点了一下主对话，正在聊的子对话从列表消失”——豁免沿子树放行，
 * 被折叠链上的中间父行也会一并展示（面包屑式显形）。
 */
export function expandChildRows(
  orderedTopLevel: Session[],
  childrenByParentId: Map<string, Session[]>,
  options: { foldedParentIds?: ReadonlySet<string>; alwaysShowPaths?: ReadonlySet<string> } = {},
): Session[] {
  if (childrenByParentId.size === 0) return orderedTopLevel;
  const { foldedParentIds, alwaysShowPaths } = options;
  const result: Session[] = [];

  // 展示挂载点重算：子行默认挂在真实父级名下；真实深度超过 MAX_LINEAGE_DEPTH
  // 的历史深层后代改挂到第一层祖先名下（钉在第三层），既不丢会话也不突破三层。
  const parentOf = new Map<string, string>();
  for (const [parentId, children] of childrenByParentId) {
    for (const child of children) {
      if (child.sessionId) parentOf.set(child.sessionId, parentId);
    }
  }
  const depthCache = new Map<string, number>();
  const depthOf = (sessionId: string): number => {
    const cached = depthCache.get(sessionId);
    if (cached !== undefined) return cached;
    // 沿真实父级向上数层；父级不在子行表里即到顶（顶层行），visited 防环。
    const seen = new Set<string>([sessionId]);
    let depth = 0;
    let current = sessionId;
    while (true) {
      const parent = parentOf.get(current);
      if (!parent || seen.has(parent)) break;
      seen.add(parent);
      current = parent;
      depth += 1;
    }
    depthCache.set(sessionId, depth);
    return depth;
  };
  const effectiveChildren = new Map<string, Session[]>();
  for (const children of childrenByParentId.values()) {
    for (const child of children) {
      const childId = child.sessionId;
      if (!childId) continue;
      const seen = new Set<string>([childId]);
      let target = childId;
      let effectiveParent: string | null = null;
      while (true) {
        const parent = parentOf.get(target);
        if (!parent || seen.has(parent)) break;
        seen.add(parent);
        if (depthOf(parent) + 1 <= MAX_LINEAGE_DEPTH) {
          effectiveParent = parent;
          break;
        }
        target = parent;
      }
      if (!effectiveParent) continue; // 谱系环等脏数据：末尾兜底扫兜住
      const list = effectiveChildren.get(effectiveParent) || [];
      list.push(child);
      effectiveChildren.set(effectiveParent, list);
    }
  }
  for (const list of effectiveChildren.values()) list.sort(compareByModifiedDesc);
  const mountedPaths = new Set<string>();
  for (const list of effectiveChildren.values()) {
    for (const child of list) mountedPaths.add(child.path);
  }

  // 折叠豁免沿子树放行：主对话被折叠时，正在聊的孙辈连同其父链一起显形。
  const subtreeHasAlwaysShow = (session: Session): boolean => {
    const stack: Session[] = [session];
    const seen = new Set<string>();
    while (stack.length > 0) {
      const node = stack.pop()!;
      if (alwaysShowPaths?.has(node.path)) return true;
      const id = node.sessionId || '';
      if (!id || seen.has(id)) continue;
      seen.add(id);
      for (const grandchild of effectiveChildren.get(id) || []) stack.push(grandchild);
    }
    return false;
  };

  const emitRow = (session: Session, annotation: { childOfSessionId: string; childDepth: number } | null) => {
    const id = session.sessionId || '';
    const base: Session = annotation ? { ...session, ...annotation } : session;
    const children = id ? effectiveChildren.get(id) : undefined;
    if (!id || !children || children.length === 0) {
      result.push(base);
      return;
    }
    const parentIndex = result.length;
    result.push({ ...base, hasChildSessions: true });
    const folded = foldedParentIds?.has(id) === true;
    let hidden = 0;
    const childDepth = (annotation?.childDepth ?? 0) + 1;
    for (const child of children) {
      if (folded && !alwaysShowPaths?.has(child.path) && !subtreeHasAlwaysShow(child)) {
        hidden += 1;
        continue;
      }
      emitRow(child, { childOfSessionId: id, childDepth });
    }
    if (folded && hidden > 0) {
      // 折叠且确有隐藏：把隐藏数写回父行克隆（展示在助手符号上）
      result[parentIndex] = { ...result[parentIndex]!, foldedChildCount: hidden };
    }
  };
  for (const session of orderedTopLevel) emitRow(session, null);

  // 兜底：谱系环等脏数据会让个别子行找不到合法挂载点（不在任何 effectiveChildren
  // 桶里，折叠豁免扫不扫都到不了它）；按顶层行补在末尾，宁可顺序异常也不丢会话。
  // 注意折叠隐藏的子行有合法挂载点，不在此列——它们靠展开父行找回。
  for (const children of childrenByParentId.values()) {
    for (const child of children) {
      if (!mountedPaths.has(child.path)) result.push(child);
    }
  }
  return result;
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
  options: BuildSessionSectionsOptions & SessionChildFoldOptions = {},
): SessionSection[] {
  const includePinned = options.includePinned !== false;
  const pinned = includePinned
    ? sessions.filter(isPinnedSession).sort(comparePinned)
    : [];
  // 谱系拆分先于日期分组：子对话跟随主对话的日期组，不因自身更新被分到别组。
  const regularSplit = splitChildSessions(sessions.filter(session => !isPinnedSession(session)));

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
  for (const session of regularSplit.topLevel) {
    dateGroups[getSessionDateGroup(session.modified, now)].push(session);
  }

  // Sort within each group: newest modified first
  for (const group of DATE_GROUP_ORDER) {
    dateGroups[group].sort(compareByModifiedDesc);
  }

  for (const group of DATE_GROUP_ORDER) {
    const items = expandChildRows(dateGroups[group], regularSplit.childrenByParentId, options);
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

/**
 * 单个会话（或待建会话身份）归属的项目组 id，与 groupSessionsByProject 的
 * 键及合流规则一致：mount 身份 → mount 键；目录身份若与某挂载的 nativeRoot
 * 一致则合流到该 mount 键，否则目录键。无可靠身份返回 null。
 *
 * 供列表推断「当前聊天所在分组」以驱动展开/折叠规则。
 */
export function resolveSessionProjectGroupId(
  identity: { cwd?: string | null; workspaceMountId?: string | null } | null | undefined,
  studios?: ProjectGroupStudioHint[] | null,
): string | null {
  if (!identity) return null;
  const mountId = typeof identity.workspaceMountId === 'string' && identity.workspaceMountId.trim()
    ? identity.workspaceMountId.trim()
    : null;
  const cwd = typeof identity.cwd === 'string' && identity.cwd.trim() ? identity.cwd : null;
  if (mountId) return `mount:${mountId}`;
  if (!cwd) return null;
  for (const studio of studios ?? []) {
    const root = typeof studio?.nativeRootPath === 'string' && studio.nativeRootPath.trim()
      ? studio.nativeRootPath
      : null;
    if (root && isSameWorkspacePath(cwd, root)) return `mount:${studio.mountId}`;
  }
  const normalized = normalizeWorkspacePath(cwd) || cwd;
  return `dir:${normalized}`;
}

// ── 项目分组（左栏列表显示全部项目：项目 → 日期两级分组） ──

export interface ProjectGroup {
  /** 稳定分组键：mount:<mountId> 或 dir:<normalizedPath>；双形态合流后取 mount 键。 */
  id: string;
  /** 项目显示名：mount label > 目录 basename。 */
  title: string;
  /** mount 身份（合流后保留）；目录形态项目为 null。 */
  mountId: string | null;
  /** 目录身份（合流后保留目录侧原始 cwd）；mount 专属项目为 null。 */
  rootPath: string | null;
  /** 组内最新活动时间，用于组间按最近活动排序。 */
  latestModified: number;
  /** 组内全部非置顶会话（置顶区在列表顶部全局渲染一次）。 */
  sessions: Session[];
}

/** 合流用的挂载工作台信息（来自 store.studioWorkspaces 的最小投影）。 */
export interface ProjectGroupStudioHint {
  mountId: string;
  label?: string | null;
  nativeRootPath?: string | null;
}

function dirBasename(normalizedPath: string): string {
  const parts = normalizedPath.split('/').filter(Boolean);
  return parts[parts.length - 1] || normalizedPath;
}

function projectTitleForDir(normalizedPath: string, sessionLabel: string | null | undefined): string {
  return (typeof sessionLabel === 'string' && sessionLabel.trim())
    ? sessionLabel.trim()
    : dirBasename(normalizedPath);
}

/**
 * 把会话按项目身份分组，供左栏显示「全部项目」分组列表。
 *
 * 身份形态与 sessionBelongsToWorkspaceScope 一致：mount 工作台按 mountId，
 * 本地目录会话按规范化 cwd；无可靠身份（两者皆缺）的会话不显示、不删除，
 * 维持既有纪律。mount 与本地目录的双形态合流：mount 的 nativeRootPath 已知
 * 且与某目录组路径一致时（default mount 之外的 local_fs 挂载常见这种同一
 * 项目两个入口的形态），会话合入同一项目组。
 *
 * 用户配置的挂载工作台即使名下还没有对话也会生成空组（可从行内
 * 「新建对话」直接在该项目起步）；纯历史目录若从未产生会话则不显示。
 *
 * 置顶是全局手动顺序，不参与项目分组；调用方在列表顶部全局渲染置顶区。
 */
export function groupSessionsByProject(
  sessions: Session[],
  options: { studios?: ProjectGroupStudioHint[] | null; now?: Date } & SessionChildFoldOptions = {},
): ProjectGroup[] {
  type Draft = {
    id: string;
    title: string;
    mountId: string | null;
    rootPath: string | null;
    sessions: Session[];
  };
  const byKey = new Map<string, Draft>();

  const ensure = (key: string, title: string, mountId: string | null, rootPath: string | null): Draft => {
    let draft = byKey.get(key);
    if (!draft) {
      draft = { id: key, title, mountId, rootPath, sessions: [] };
      byKey.set(key, draft);
    }
    return draft;
  };

  // mount 的 nativeRootPath → dir 键，用于双形态合流。
  const studioByMountId = new Map<string, ProjectGroupStudioHint>();
  for (const studio of options.studios ?? []) {
    if (studio?.mountId) studioByMountId.set(studio.mountId, studio);
  }

  for (const session of sessions) {
    if (isPinnedSession(session)) continue;
    const mountId = typeof session.workspaceMountId === 'string' && session.workspaceMountId.trim()
      ? session.workspaceMountId.trim()
      : null;
    const cwd = typeof session.cwd === 'string' && session.cwd.trim() ? session.cwd : null;
    if (mountId) {
      const studio = studioByMountId.get(mountId);
      const title = (studio?.label || session.workspaceLabel || mountId).trim();
      ensure(`mount:${mountId}`, title, mountId, studio?.nativeRootPath || null).sessions.push(session);
      continue;
    }
    if (cwd) {
      const normalized = normalizeWorkspacePath(cwd) || cwd;
      ensure(`dir:${normalized}`, projectTitleForDir(normalized, session.workspaceLabel), null, cwd).sessions.push(session);
    }
    // cwd 也缺失：无身份，维持不显示的纪律。
  }

  // 双形态合流：mount 组的 nativeRoot 与某目录组一致 → 会话并入 mount 组。
  for (const [mountId, studio] of studioByMountId) {
    const nativeRoot = typeof studio.nativeRootPath === 'string' && studio.nativeRootPath.trim()
      ? studio.nativeRootPath
      : null;
    if (!nativeRoot) continue;
    const normalizedRoot = normalizeWorkspacePath(nativeRoot) || nativeRoot;
    const dirKey = `dir:${normalizedRoot}`;
    const dirDraft = byKey.get(dirKey);
    if (!dirDraft) continue;
    const mountDraft = ensure(`mount:${mountId}`, (studio.label || dirDraft.title).trim(), mountId, nativeRoot);
    mountDraft.sessions.push(...dirDraft.sessions);
    mountDraft.title = (studio.label || dirDraft.title).trim();
    byKey.delete(dirKey);
  }

  const groups: ProjectGroup[] = [];
  for (const draft of byKey.values()) {
    if (draft.sessions.length === 0) continue;
    // 谱系拆分先于排序：子对话不参与顶层排序，展开时紧跟其主对话；
    // 组位置（latestModified）只由顶层会话决定，子对话活动不顶起整组。
    const split = splitChildSessions(draft.sessions);
    split.topLevel.sort(compareByModifiedDesc);
    groups.push({
      id: draft.id,
      title: draft.title,
      mountId: draft.mountId,
      rootPath: draft.rootPath,
      latestModified: split.topLevel.length > 0 ? modifiedTime(split.topLevel[0]) : 0,
      sessions: expandChildRows(split.topLevel, split.childrenByParentId, options),
    });
  }

  // 挂载工作台即使还没有对话也占一个组（可从行内「新建对话」直接起步）；
  // 空组 latestModified 为 0，自然排在有活动的项目之后。
  for (const [mountId, studio] of studioByMountId) {
    if (byKey.has(`mount:${mountId}`)) continue;
    const nativeRoot = typeof studio.nativeRootPath === 'string' && studio.nativeRootPath.trim()
      ? studio.nativeRootPath
      : null;
    const normalizedRoot = nativeRoot ? (normalizeWorkspacePath(nativeRoot) || nativeRoot) : null;
    const title = (studio.label || (normalizedRoot ? dirBasename(normalizedRoot) : mountId)).trim();
    groups.push({
      id: `mount:${mountId}`,
      title,
      mountId,
      rootPath: nativeRoot,
      latestModified: 0,
      sessions: [],
    });
  }
  groups.sort((a, b) => b.latestModified - a.latestModified || a.id.localeCompare(b.id));
  return groups;
}
