/**
 * SessionList — 侧边栏 session 列表 React 组件
 *
 * Phase 6B: 替代 sidebar-shim.ts 中的 renderSessionList / createSessionItem。
 * 通过 portal 渲染到 #sessionList，从 Zustand sessions 状态驱动。
 *
 * 布局重构（任务七~十）：
 * - 列表在数据层按当前工作台作用域过滤（mount 严格等值 / 本地目录规范化 cwd 比较），
 *   不做视觉遮盖；无可靠身份的 session 不显示、不删除，仍可被全局搜索找到。
 * - 项目/时间视图切换、项目目录式导航已从常驻左栏移除；时间分组（置顶/today/week/earlier）
 *   保留为列表组织。项目数据动作（session-project-actions/store/API）保持现状。
 * - 搜索已抽离到 Titlebar 放大镜 + 居中搜索界面（components/search/ChatSearchOverlay）。
 */

import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useStore } from '../stores';
import { lingxiFetch } from '../hooks/use-hana-fetch';
import { useI18n } from '../hooks/use-i18n';
import { formatSessionDate } from '../utils/format';
import { switchSession, archiveSession, renameSession, pinSession, reorderPinnedSessions, disposeWorkspaceSessions } from '../stores/session-actions';
import { setBrowserStateForPath } from '../stores/browser-slice';
import { sessionScopedListIncludes } from '../stores/session-slice';
import type { Session, Agent } from '../types';
import { AgentAvatar, resolveAgentDisplayInfo } from '../utils/agent-display';
import { buildSessionSections, groupSessionsByProject, resolveSessionProjectGroupId, isPinnedSession, type ProjectGroup } from './session-sections';
import { applyFolder, applyStudioWorkspace } from '../stores/desk-actions';
import { removeStudioWorkspaceWithDisposal, removeWorkspaceHistoryEntry } from '../utils/workspace-switch';
import type { SidebarSessionListRowMode } from '../../../../shared/sidebar-ui-state.ts';
import { ContextMenu, type ContextMenuItem } from '../ui/ContextMenu';
import { Overlay } from '../ui/Overlay';
import { Button } from '../ui/Button';
import { renderMarkdown } from '../utils/markdown';
import styles from './SessionList.module.css';

// 置顶区行拖拽 MIME：pinned 重排提交完整有序 sessionId 列表。
const SESSION_DRAG_MIME = 'application/x-lingxi-session-path';

type SidebarDragState =
  | { kind: 'pinned-session'; sessionPath: string; sessionId: string | null }
  | null;

// 置顶区拖拽重排时的插入指示线位置：落在目标行的上边还是下边
type PinnedDropTarget = { sessionPath: string; edge: 'before' | 'after' } | null;

interface BrowserSessionState {
  url: string | null;
  running: boolean;
  resumable: boolean;
  unavailableReason: string | null;
}

function normalizeBrowserSessionStates(data: unknown): Record<string, BrowserSessionState> {
  if (!data || typeof data !== 'object' || Array.isArray(data)) return {};
  const result: Record<string, BrowserSessionState> = {};
  for (const [sessionPath, rawState] of Object.entries(data as Record<string, unknown>)) {
    if (typeof rawState === 'string') {
      result[sessionPath] = {
        url: rawState,
        running: false,
        resumable: true,
        unavailableReason: null,
      };
      continue;
    }
    if (!rawState || typeof rawState !== 'object' || Array.isArray(rawState)) continue;
    const state = rawState as Partial<BrowserSessionState>;
    result[sessionPath] = {
      url: typeof state.url === 'string' ? state.url : null,
      running: state.running === true,
      resumable: state.resumable !== false,
      unavailableReason: typeof state.unavailableReason === 'string' ? state.unavailableReason : null,
    };
  }
  return result;
}

// ── 主组件 ──

export function SessionList() {
  return <SessionListInner />;
}

// ── 内部组件 ──

function SessionListInner() {
  const { t } = useI18n();
  const sessions = useStore(s => s.sessions);
  const currentSessionPath = useStore(s => s.currentSessionPath);
  const pendingSessionSwitchPath = useStore(s => s.pendingSessionSwitchPath);
  const pendingNewSession = useStore(s => s.pendingNewSession);
  const agents = useStore(s => s.agents);
  const streamingSessions = useStore(s => s.streamingSessions);
  const unreadOutputSessionPaths = useStore(s => s.unreadOutputSessionPaths);
  const failedSessions = useStore(s => s.failedSessions);
  const browserBySession = useStore(s => s.browserBySession);
  const metaRecovery = useStore(s => s.metaRecovery);
  // 侧边栏 UI 偏好归 store：本组件有多个实例（主侧栏 / 悬浮侧栏），
  // 重挂载时直接读已加载的值，不再各自拉取、也就没有默认双行的首帧。
  const sidebarUiPrefs = useStore(s => s.sidebarUiPrefs);
  const sessionListRowMode: SidebarSessionListRowMode = sidebarUiPrefs.sessionList.rowMode;

  // ── 项目分组（全部项目可见；不再按当前工作台过滤）──
  // 项目 = 会话的工作台身份（mount 或本地目录），mount 的 nativeRoot 与某目录
  // 一致时双形态合流；组内按日期分组，组间按最近活动排序。工作台面板的激活
  // 身份仍由会话切换流程（resetDeskForSessionWorkspace）跟随当前对话切换。
  // ── 「执行中」分组 ──
  // 多条对话同时在跑任务时集中在一个组：固定排在置顶区下面、始终展开、空了自动隐藏。
  // 成员 = 非置顶且（正在输出 streaming 或 后台完成未查看 unread）；
  // 用户点进查看后 unread 清除，会话自动回到原项目组。
  // 置顶会话始终留在顶部置顶区（置顶是用户手动固定的位置，不随执行状态临时搬动；
  // 执行状态由行首旋转图标/绿点表达，置顶区标题也不会因此消失）。
  const activeRunningSessions = useMemo(() => {
    const state = useStore.getState();
    return sessions
      .filter(s =>
        !isPinnedSession(s)
        && (sessionScopedListIncludes(state, streamingSessions, s.path)
            || sessionScopedListIncludes(state, unreadOutputSessionPaths, s.path)))
      .sort((a, b) => (Date.parse(b.modified || '') || 0) - (Date.parse(a.modified || '') || 0));
  }, [sessions, streamingSessions, unreadOutputSessionPaths]);
  const activeRunningPaths = useMemo(
    () => new Set(activeRunningSessions.map(s => s.path)),
    [activeRunningSessions],
  );
  // 执行中的会话由「执行中」组独家展示，置顶区与项目组里不再重复出现。
  const idleSessions = useMemo(
    () => sessions.filter(s => !activeRunningPaths.has(s.path)),
    [sessions, activeRunningPaths],
  );
  const studioWorkspaces = useStore(s => s.studioWorkspaces);
  // 子对话折叠（点击主对话收起/展开）：本地状态按主对话 sessionId 记忆；
  // 当前正在聊的会话永远展示（即使其主对话被折叠），避免"点一下主对话，
  // 正在聊的支线从列表里消失"。
  const [foldedParents, setFoldedParents] = useState<Record<string, boolean>>({});
  const toggleChildrenFold = useCallback((session: Session) => {
    const key = session.sessionId || session.path;
    setFoldedParents(prev => ({ ...prev, [key]: !prev[key] }));
  }, []);
  const foldedParentIds = useMemo(
    () => new Set(Object.entries(foldedParents).filter(([, folded]) => folded).map(([key]) => key)),
    [foldedParents],
  );
  const alwaysShowSessionPaths = useMemo(() => {
    const paths = new Set<string>();
    const current = pendingSessionSwitchPath || currentSessionPath;
    if (current) paths.add(current);
    return paths;
  }, [currentSessionPath, pendingSessionSwitchPath]);
  const projectGroups = useMemo(
    () => groupSessionsByProject(idleSessions, {
      studios: studioWorkspaces,
      foldedParentIds,
      alwaysShowPaths: alwaysShowSessionPaths,
    }),
    [idleSessions, studioWorkspaces, foldedParentIds, alwaysShowSessionPaths],
  );

  const [browserSessions, setBrowserSessions] = useState<Record<string, BrowserSessionState>>({});
  const [dragState, setDragState] = useState<SidebarDragState>(null);
  const [pinnedDropTarget, setPinnedDropTarget] = useState<PinnedDropTarget>(null);
  const closingBrowserSessionsRef = useRef(new Set<string>());

  const setVisibleBrowserSessions = useCallback((data: unknown) => {
    const states = normalizeBrowserSessionStates(data);
    for (const sessionPath of closingBrowserSessionsRef.current) {
      delete states[sessionPath];
    }
    setBrowserSessions(states);
  }, []);

  // Fetch browser sessions (re-fetch when browser state changes)
  useEffect(() => {
    let cancelled = false;
    if (sessions.length === 0) {
      setBrowserSessions({});
      return;
    }
    lingxiFetch('/api/browser/session-states')
      .then(r => r.json())
      .then(data => {
        if (!cancelled) setVisibleBrowserSessions(data);
      })
      .catch(err => console.warn('[sessions] fetch browser sessions failed:', err));
    return () => {
      cancelled = true;
    };
  }, [sessions, browserBySession, setVisibleBrowserSessions]);

  const handleCloseBrowserSession = useCallback(async (sessionPath: string) => {
    closingBrowserSessionsRef.current.add(sessionPath);
    setBrowserSessions(prev => {
      const next = { ...prev };
      delete next[sessionPath];
      return next;
    });
    try {
      const res = await lingxiFetch('/api/browser/close-session', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sessionPath }),
      });
      const data = await res.json();
      setBrowserStateForPath(sessionPath, { running: false, url: null, thumbnail: null });
      closingBrowserSessionsRef.current.delete(sessionPath);
      if (data?.sessions) {
        setBrowserSessions(normalizeBrowserSessionStates(data.sessions));
      }
    } catch (err) {
      closingBrowserSessionsRef.current.delete(sessionPath);
      console.warn('[sessions] close browser session failed:', err);
    }
  }, []);

  const clearDragState = useCallback(() => {
    setDragState(null);
    setPinnedDropTarget(null);
  }, []);

  // ── 置顶区内拖拽重排 ──

  const handlePinnedDragStart = useCallback((event: React.DragEvent, session: Session) => {
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData(SESSION_DRAG_MIME, session.path);
    setDragState({
      kind: 'pinned-session',
      sessionPath: session.path,
      sessionId: session.sessionId || null,
    });
  }, []);

  const handlePinnedDragOver = useCallback((event: React.DragEvent, session: Session) => {
    if (dragState?.kind !== 'pinned-session') return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = 'move';
    const rect = event.currentTarget.getBoundingClientRect();
    const edge = event.clientY - rect.top < rect.height / 2 ? 'before' : 'after';
    setPinnedDropTarget({ sessionPath: session.path, edge });
  }, [dragState]);

  const handlePinnedDragLeave = useCallback((event: React.DragEvent) => {
    const next = event.relatedTarget as Node | null;
    if (next && event.currentTarget.contains(next)) return;
    setPinnedDropTarget(current => (
      current && current.sessionPath === (event.currentTarget as HTMLElement).dataset.pinnedSessionPath
        ? null
        : current
    ));
  }, []);

  const handlePinnedDrop = useCallback((
    event: React.DragEvent,
    target: Session,
    pinnedItems: Session[],
  ) => {
    event.preventDefault();
    event.stopPropagation();
    const draggedPath = dragState?.kind === 'pinned-session' ? dragState.sessionPath : null;
    const edge = pinnedDropTarget?.sessionPath === target.path ? pinnedDropTarget.edge : 'before';
    clearDragState();
    if (!draggedPath || draggedPath === target.path) return;
    // 缺 sessionId 就没有可提交的身份，整区不重排（门控见 pinnedReorderEnabled）
    if (pinnedItems.some(session => !session.sessionId)) return;
    const dragged = pinnedItems.find(session => session.path === draggedPath);
    if (!dragged) return;
    const ordered = pinnedItems.filter(session => session.path !== draggedPath);
    const targetIndex = ordered.findIndex(session => session.path === target.path);
    if (targetIndex < 0) return;
    ordered.splice(edge === 'after' ? targetIndex + 1 : targetIndex, 0, dragged);
    void reorderPinnedSessions(ordered.map(session => session.sessionId as string));
  }, [clearDragState, dragState, pinnedDropTarget]);

  // 归档入口统一走这里：主对话名下有子对话时先弹选择框（一起归档 / 仅主对话），
  // 无子对话直接归档。子对话数以本地列表为准，服务端仍会兑底拦截。
  const [archiveChoice, setArchiveChoice] = useState<{ session: Session; childCount: number } | null>(null);
  const requestArchive = useCallback((session: Session) => {
    const state = useStore.getState();
    const parentSessionId = session.sessionId || null;
    if (!parentSessionId) {
      void archiveSession(session.path);
      return;
    }
    const children = (state.sessions || []).filter((item: Session) => (
      item.sessionId
      && item.sessionId !== parentSessionId
      && item.path !== session.path
      && item.forkedFrom?.sessionId === parentSessionId
    ));
    if (children.length === 0) {
      void archiveSession(session.path);
      return;
    }
    setArchiveChoice({ session, childCount: children.length });
  }, []);

  const activeSessionPath = pendingSessionSwitchPath || currentSessionPath;
  const renderSessionItem = (
    s: Session,
    options: { draggable?: boolean; onDragStart?: (event: React.DragEvent, session: Session) => void } = {},
  ) => (
    <SessionItem
      key={s.path}
      session={s}
      isActive={!pendingNewSession && s.path === activeSessionPath}
      isPending={!pendingNewSession && pendingSessionSwitchPath === s.path}
      isStreaming={sessionScopedListIncludes(useStore.getState(), streamingSessions, s.path)}
      hasUnreadOutput={sessionScopedListIncludes(useStore.getState(), unreadOutputSessionPaths, s.path)}
      hasFailed={sessionScopedListIncludes(useStore.getState(), failedSessions, s.path)}
      isPinned={!!s.pinnedAt}
      isChildRow={!!s.childOfSessionId}
      hasChildSessions={!!s.hasChildSessions}
      childrenFolded={!!s.hasChildSessions && !!(s.sessionId && foldedParents[s.sessionId])}
      foldedChildCount={s.foldedChildCount || 0}
      onToggleChildren={toggleChildrenFold}
      agents={agents}
      browserState={browserSessions[s.path] || null}
      rowMode={sessionListRowMode}
      onCloseBrowser={handleCloseBrowserSession}
      onArchiveRequest={requestArchive}
      draggable={options.draggable === true && s.agentDeleted !== true}
      onDragStart={options.onDragStart}
      onDragEnd={clearDragState}
    />
  );

  // 置顶行：可拖拽重排，行内上/下半区决定插入位。整区任一行缺 sessionId 就整体禁用，
  // 因为提交的是完整有序 sessionId 列表，缺一个就无法表达完整顺序。
  const renderPinnedSessionItem = (s: Session, pinnedItems: Session[]) => {
    const reorderable = pinnedItems.length > 1 && pinnedItems.every(item => !!item.sessionId);
    const indicator = pinnedDropTarget?.sessionPath === s.path
      ? (pinnedDropTarget.edge === 'before'
        ? styles.pinnedDropIndicatorBefore
        : styles.pinnedDropIndicatorAfter)
      : '';
    return (
      <div
        key={s.path}
        className={`${styles.pinnedRow}${indicator ? ` ${indicator}` : ''}`}
        data-pinned-session-path={s.path}
        onDragOver={reorderable ? (event) => handlePinnedDragOver(event, s) : undefined}
        onDragLeave={reorderable ? handlePinnedDragLeave : undefined}
        onDrop={reorderable ? (event) => handlePinnedDrop(event, s, pinnedItems) : undefined}
      >
        {renderSessionItem(s, { draggable: reorderable, onDragStart: handlePinnedDragStart })}
      </div>
    );
  };

  // 全局置顶区：置顶是跨项目的手动顺序，保持在列表顶部渲染一次。
  const allSections = buildSessionSections(idleSessions, { mode: 'time' });
  const pinnedSection = allSections.find(section => section.kind === 'pinned');
  const hasPinnedItems = !!(pinnedSection && pinnedSection.items.length > 0);
  // 空状态只在真没对话时出现；全部会话都在执行中（项目组暂空）时不算空，
  // 只有置顶会话时同样不算——置顶条仍要渲染。
  const showEmptyState = sessions.length === 0
    || (projectGroups.length === 0 && activeRunningSessions.length === 0 && !hasPinnedItems);

  const pinnedContent = pinnedSection && pinnedSection.items.length > 0 ? (
    <section className={styles.pinnedSection}>
      <SectionTitle className={styles.pinnedSectionTitle}>
        <span>{t(pinnedSection.titleKey)}</span>
        <PinIcon />
      </SectionTitle>
      {pinnedSection.items.map(s => renderPinnedSessionItem(s, pinnedSection.items))}
    </section>
  ) : null;

  // 「执行中」组：常驻展开，不给折叠交互；组头用脉冲点表达活跃，右侧只留计数。
  const runningContent = activeRunningSessions.length > 0 ? (
    <section className={styles.projectSection} data-running-group="true">
      <div className={styles.projectHeader} data-collapsed="false">
        <div className={styles.projectHeaderStatic}>
          <span className={styles.runningGroupPulse} aria-hidden="true" />
          <span className={styles.projectSectionName}>{t('sidebar.runningGroup')}</span>
          <span className={styles.projectSectionCount}>{activeRunningSessions.length}</span>
        </div>
      </div>
      {activeRunningSessions.map(s => renderSessionItem(s))}
    </section>
  ) : null;

  // ── 折叠规则：跟随当前聊天所在分组 ──
  // 当前会话（或待建会话身份）所在的分组展开，其余分组默认折叠。
  // 点击标题在默认规则上取反，记录在本地临时覆盖；切换到别的分组的对话后
  // 重置——展开状态随之转移到新分组。
  const selectedWorkspaceMountId = useStore(s => s.selectedWorkspaceMountId);
  const selectedFolder = useStore(s => s.selectedFolder);
  const activeGroupId = useMemo(() => {
    const current = currentSessionPath
      ? sessions.find(s => s.path === currentSessionPath)
      : null;
    if (current) return resolveSessionProjectGroupId(current, studioWorkspaces);
    // 待建会话（新对话/欢迎态）：以选定的工作台身份为准。
    return resolveSessionProjectGroupId(
      { workspaceMountId: selectedWorkspaceMountId, cwd: selectedFolder },
      studioWorkspaces,
    );
  }, [currentSessionPath, selectedFolder, selectedWorkspaceMountId, sessions, studioWorkspaces]);

  const [manualOverrides, setManualOverrides] = useState<Record<string, boolean>>({});
  useEffect(() => {
    // 聊天切到别的分组后，手动覆盖清空，回到默认规则（新组展开、旧组折叠）。
    setManualOverrides({});
  }, [activeGroupId]);

  const isProjectCollapsed = useCallback((group: ProjectGroup) =>
    manualOverrides[group.id] ?? (group.id !== activeGroupId),
    [activeGroupId, manualOverrides]);
  const toggleProjectCollapsed = useCallback((groupId: string) => {
    setManualOverrides(m => {
      // 先还原当前折叠状态（手动覆盖优先，默认规则=非活跃组折叠），取反后写入。
      // 此前实现把「当前展开状态」原样存回覆盖表，只有首次点击碰巧生效，
      // 第二次起值恒定不变，表现为点了没反应。
      const collapsed = m[groupId] ?? (groupId !== activeGroupId);
      return { ...m, [groupId]: !collapsed }; // 展开中 → 折叠；折叠中 → 展开
    });
  }, [activeGroupId]);

  // 项目行 hover 动作：新建对话 / 三点菜单。
  const [projectMenu, setProjectMenu] = useState<{ x: number; y: number; group: ProjectGroup } | null>(null);
  const handleProjectNewChat = useCallback((group: ProjectGroup) => {
    if (group.mountId) {
      const studio = (useStore.getState().studioWorkspaces || [])
        .find((item: any) => item.mountId === group.mountId);
      void applyStudioWorkspace({
        mountId: group.mountId,
        label: group.title,
        nativeRootPath: studio?.nativeRootPath || group.rootPath || null,
      });
      return;
    }
    if (group.rootPath) void applyFolder(group.rootPath);
  }, []);
  const handleProjectRemove = useCallback((group: ProjectGroup) => {
    if (group.mountId) void removeStudioWorkspaceWithDisposal(group.mountId);
    else if (group.rootPath) void removeWorkspaceHistoryEntry(group.rootPath);
  }, []);
  const handleProjectArchiveAll = useCallback(async (group: ProjectGroup) => {
    const result = await disposeWorkspaceSessions(
      { workspaceMountId: group.mountId, cwd: group.rootPath ?? undefined },
      'archive',
    );
    if (result) {
      useStore.getState().addToast(
        t('sidebar.projectArchiveDone', { count: String(result.disposed) }),
        'success',
        6000,
      );
    }
  }, [t]);

  const projectMenuItems = useMemo<ContextMenuItem[]>(() => {
    if (!projectMenu) return [];
    const { group } = projectMenu;
    return [
      {
        label: t('sidebar.projectRemove'),
        danger: true,
        action: () => handleProjectRemove(group),
      },
      {
        label: t('sidebar.projectArchiveAll'),
        action: () => void handleProjectArchiveAll(group),
      },
    ];
  }, [handleProjectArchiveAll, handleProjectRemove, projectMenu, t]);

  const projectContent = projectGroups.map(group => {
    const projectCollapsed = isProjectCollapsed(group);
    return (
      <section key={group.id} className={styles.projectSection} data-project-id={group.id}>
        <div
          className={styles.projectHeader}
          data-collapsed={projectCollapsed ? 'true' : 'false'}
        >
          <button
            type="button"
            className={styles.projectHeaderMain}
            aria-expanded={!projectCollapsed}
            aria-label={group.title}
            onClick={() => toggleProjectCollapsed(group.id)}
          >
            <FolderGlyph collapsed={projectCollapsed} />
            <span className={styles.projectSectionName} title={group.title}>{group.title}</span>
            <span className={styles.projectSectionCount}>{group.sessions.length}</span>
          </button>
          <div className={styles.projectHeaderActions}>
            <button
              type="button"
              className={styles.projectActionButton}
              aria-label={t('sidebar.projectNewChat')}
              title={t('sidebar.projectNewChat')}
              onClick={(event) => {
                event.stopPropagation();
                handleProjectNewChat(group);
              }}
            >
              <NewChatGlyph />
            </button>
            <button
              type="button"
              className={styles.projectActionButton}
              aria-label={t('sidebar.projectMore')}
              title={t('sidebar.projectMore')}
              onClick={(event) => {
                event.stopPropagation();
                const rect = event.currentTarget.getBoundingClientRect();
                setProjectMenu({ x: rect.left, y: rect.bottom + 4, group });
              }}
            >
              <DotsGlyph />
            </button>
          </div>
        </div>
        {!projectCollapsed && group.sessions.map(s => renderSessionItem(s))}
      </section>
    );
  });

  const content = showEmptyState ? (
    <div className={styles.sessionEmpty}>
      {metaRecovery?.degraded ? t('sidebar.metaRecoveryEmpty') : t('sidebar.empty')}
    </div>
  ) : (
    <>
      {pinnedContent}
      {runningContent}
      {projectContent}
      {projectMenu && (
        <ContextMenu
          items={projectMenuItems}
          position={{ x: projectMenu.x, y: projectMenu.y }}
          onClose={() => setProjectMenu(null)}
        />
      )}
      {archiveChoice && (
        <SessionArchiveChoiceDialog
          session={archiveChoice.session}
          childCount={archiveChoice.childCount}
          onClose={() => setArchiveChoice(null)}
        />
      )}
    </>
  );

  return (
    <>
      <div className={styles.sessionListScroller}>
        {content}
      </div>
    </>
  );
}

function SectionTitle({
  children,
  className = '',
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={`${styles.sessionSectionTitle}${className ? ` ${className}` : ''}`}>
      <div className={styles.sessionSectionTitleMain}>{children}</div>
    </div>
  );
}

function FolderGlyph({ collapsed }: { collapsed: boolean }) {
  // 展开 = 打开的文件夹；折叠 = 闭合的文件夹。状态由图标本身表达。
  return collapsed ? (
    <svg className={styles.projectTitleIcon} width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </svg>
  ) : (
    <svg className={styles.projectTitleIcon} width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" />
    </svg>
  );
}

// 与输入框下方「新建对话」按钮同一枚图标：对话气泡内嵌加号。
function NewChatGlyph() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
      <path d="M9 10h6" />
      <path d="M12 7v6" />
    </svg>
  );
}

function DotsGlyph() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <circle cx="5" cy="12" r="1.6" />
      <circle cx="12" cy="12" r="1.6" />
      <circle cx="19" cy="12" r="1.6" />
    </svg>
  );
}

function PinIcon() {
  return (
    <svg className={styles.pinnedTitleIcon} width="11" height="11" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path fill="currentColor" d="M15.9894 4.9502L16.52 4.42014V4.42014L15.9894 4.9502ZM19.0716 8.03562L18.541 8.56568L19.0716 8.03562ZM8.73837 19.429L8.20777 19.9591L8.73837 19.429ZM4.62169 15.3081L5.15229 14.7781L4.62169 15.3081ZM17.5669 14.9943L17.3032 14.2922L17.5669 14.9943ZM15.6498 15.7146L15.9136 16.4167H15.9136L15.6498 15.7146ZM8.3322 8.38177L7.62798 8.12375L8.3322 8.38177ZM9.02665 6.48636L9.73087 6.74438V6.74438L9.02665 6.48636ZM5.84504 10.6735L6.04438 11.3965L5.84504 10.6735ZM7.30167 10.1351L6.86346 9.52646L6.86346 9.52646L7.30167 10.1351ZM7.67582 9.79038L8.24665 10.2768H8.24665L7.67582 9.79038ZM14.251 16.3805L14.742 16.9475L14.742 16.9475L14.251 16.3805ZM13.3806 18.2012L12.6574 18.0022V18.0022L13.3806 18.2012ZM13.9169 16.7466L13.3075 16.3094L13.3075 16.3094L13.9169 16.7466ZM2.71846 12.7552L1.96848 12.76L1.96848 12.76L2.71846 12.7552ZM2.93045 11.9521L2.28053 11.5778H2.28053L2.93045 11.9521ZM11.3052 21.3431L11.3064 20.5931H11.3064L11.3052 21.3431ZM12.0933 21.1347L11.7215 20.4833L11.7215 20.4833L12.0933 21.1347ZM11.6973 2.03606L11.8588 2.76845L11.6973 2.03606ZM1.4694 21.4699C1.17666 21.763 1.1769 22.2379 1.46994 22.5306C1.76298 22.8233 2.23786 22.8231 2.5306 22.5301L1.4694 21.4699ZM7.18383 17.8721C7.47657 17.5791 7.47633 17.1042 7.18329 16.8114C6.89024 16.5187 6.41537 16.5189 6.12263 16.812L7.18383 17.8721ZM15.4588 5.48026L18.541 8.56568L19.6022 7.50556L16.52 4.42014L15.4588 5.48026ZM9.26897 18.8989L5.15229 14.7781L4.09109 15.8382L8.20777 19.9591L9.26897 18.8989ZM17.3032 14.2922L15.386 15.0125L15.9136 16.4167L17.8307 15.6964L17.3032 14.2922ZM9.03642 8.63979L9.73087 6.74438L8.32243 6.22834L7.62798 8.12375L9.03642 8.63979ZM6.04438 11.3965C6.75583 11.2003 7.29719 11.0625 7.73987 10.7438L6.86346 9.52646C6.69053 9.65097 6.46601 9.72428 5.6457 9.95044L6.04438 11.3965ZM7.62798 8.12375C7.33502 8.92332 7.24338 9.14153 7.10499 9.30391L8.24665 10.2768C8.60041 9.86175 8.7823 9.33337 9.03642 8.63979L7.62798 8.12375ZM7.73987 10.7438C7.92696 10.6091 8.09712 10.4523 8.24665 10.2768L7.10499 9.30391C7.0337 9.38757 6.9526 9.46229 6.86346 9.52646L7.73987 10.7438ZM15.386 15.0125C14.697 15.2714 14.1716 15.4571 13.76 15.8135L14.742 16.9475C14.9028 16.8082 15.1192 16.7152 15.9136 16.4167L15.386 15.0125ZM14.1037 18.4001C14.329 17.5813 14.4021 17.3569 14.5263 17.1838L13.3075 16.3094C12.9902 16.7517 12.8529 17.2919 12.6574 18.0022L14.1037 18.4001ZM13.76 15.8135C13.5903 15.9605 13.4384 16.1269 13.3075 16.3094L14.5263 17.1838C14.5887 17.0968 14.6611 17.0175 14.742 16.9475L13.76 15.8135ZM5.15229 14.7781C4.50615 14.1313 4.06799 13.691 3.78366 13.3338C3.49835 12.9753 3.46889 12.8201 3.46845 12.7505L1.96848 12.76C1.97215 13.3422 2.26127 13.8297 2.61002 14.2679C2.95976 14.7073 3.47115 15.2176 4.09109 15.8382L5.15229 14.7781ZM5.6457 9.95044C4.80048 10.1835 4.10396 10.3743 3.58296 10.5835C3.06341 10.792 2.57116 11.0732 2.28053 11.5778L3.58038 12.3264C3.615 12.2663 3.71693 12.146 4.1418 11.9755C4.56523 11.8055 5.16337 11.6394 6.04438 11.3965L5.6457 9.95044ZM3.46845 12.7505C3.46751 12.6016 3.50616 12.4553 3.58038 12.3264L2.28053 11.5778C2.07354 11.9372 1.96586 12.3452 1.96848 12.76L3.46845 12.7505ZM8.20777 19.9591C8.83164 20.5836 9.34464 21.0987 9.78647 21.4506C10.227 21.8015 10.7179 22.0922 11.3041 22.0931L11.3064 20.5931C11.2369 20.593 11.0814 20.5644 10.721 20.2773C10.3618 19.9912 9.91923 19.5499 9.26897 18.8989L8.20777 19.9591ZM12.6574 18.0022C12.4133 18.8897 12.2462 19.4924 12.0751 19.9188C11.9033 20.3467 11.7821 20.4487 11.7215 20.4833L12.465 21.7861C12.974 21.4956 13.2573 21.0004 13.4671 20.4775C13.6776 19.9532 13.8694 19.2516 14.1037 18.4001L12.6574 18.0022ZM11.3041 22.0931C11.7112 22.0937 12.1114 21.9879 12.465 21.7861L11.7215 20.4833C11.595 20.5555 11.4519 20.5933 11.3064 20.5931L11.3041 22.0931ZM18.541 8.56568C19.6045 9.63022 20.3403 10.3695 20.7917 10.9788C21.2353 11.5774 21.2863 11.8959 21.2321 12.1464L22.6982 12.4634C22.8881 11.5854 22.5382 10.8162 21.9969 10.0857C21.4635 9.36592 20.6305 8.53486 19.6022 7.50556L18.541 8.56568ZM17.8307 15.6964C19.1921 15.1849 20.294 14.773 21.0771 14.3384C21.8718 13.8973 22.5083 13.3416 22.6982 12.4634L21.2321 12.1464C21.178 12.3968 21.0001 12.6655 20.3491 13.0268C19.6865 13.3946 18.7112 13.7632 17.3032 14.2922L17.8307 15.6964ZM16.52 4.42014C15.4841 3.3832 14.6481 2.54353 13.9246 2.00638C13.1908 1.46165 12.4175 1.10912 11.5357 1.30367L11.8588 2.76845C12.1086 2.71335 12.4277 2.7633 13.0304 3.21075C13.6433 3.66579 14.3876 4.40801 15.4588 5.48026L16.52 4.42014ZM9.73087 6.74438C10.2525 5.32075 10.6161 4.33403 10.9812 3.66315C11.3402 3.00338 11.609 2.82357 11.8588 2.76845L11.5357 1.30367C10.654 1.49819 10.1005 2.14332 9.66362 2.94618C9.23278 3.73793 8.82688 4.85154 8.32243 6.22834L9.73087 6.74438ZM2.5306 22.5301L7.18383 17.8721L6.12263 16.812L1.4694 21.4699L2.5306 22.5301Z" />
    </svg>
  );
}

function BrowserStatusIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="4" y="5" width="16" height="14" rx="2" />
      <path d="M4 9h16" />
      <path d="M8 7h.01M11 7h.01" />
    </svg>
  );
}

// ── Session Item ──

const SessionItem = memo(function SessionItem({ session: s, isActive, isPending, isStreaming, isPinned, isChildRow, hasChildSessions, childrenFolded, foldedChildCount, hasUnreadOutput, hasFailed, agents, browserState, rowMode, onCloseBrowser, onArchiveRequest, onToggleChildren, draggable = false, onDragStart, onDragEnd }: {
  session: Session;
  isActive: boolean;
  isPending: boolean;
  isStreaming: boolean;
  isPinned: boolean;
  /** 谱系子行：缩进两级展示（主对话的子对话） */
  isChildRow?: boolean;
  /** 主对话行：名下挂有子对话（展示折叠开关） */
  hasChildSessions?: boolean;
  /** 主对话行：子对话当前处于折叠态 */
  childrenFolded?: boolean;
  /** 主对话行：被折叠隐藏的子对话数（折叠时展示在助手符号上） */
  foldedChildCount?: number;
  hasUnreadOutput: boolean;
  hasFailed: boolean;
  agents: Agent[];
  browserState: BrowserSessionState | null;
  rowMode: SidebarSessionListRowMode;
  onCloseBrowser: (sessionPath: string) => void;
  /** 归档入口：子对话存在时由上层弹选择框 */
  onArchiveRequest: (session: Session) => void;
  /** 主对话点击折叠开关：收起/展开子对话 */
  onToggleChildren: (session: Session) => void;
  draggable?: boolean;
  onDragStart?: (event: React.DragEvent, session: Session) => void;
  onDragEnd?: () => void;
}) {
  const { t } = useI18n();
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState('');
  const [menuPosition, setMenuPosition] = useState<{ x: number; y: number } | null>(null);
  const [summaryPreviewPosition, setSummaryPreviewPosition] = useState<{ x: number; y: number } | null>(null);
  const [browserMenuPosition, setBrowserMenuPosition] = useState<{ x: number; y: number } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const isDeletedAgentSession = s.agentDeleted === true;
  // 循环任务状态（running/paused 才会有，来自 loop_status WS + 冷启动注入）。列表徽章据此渲染。
  const loopStatus = useStore((st) => (s.sessionId ? st.loopStatusBySession[s.sessionId] : undefined));
  // 后台会话待用户决策的挂起块（ask_user 提问 / 工具批准 / 应用控制批准…）。
  // 写入端以 sessionScopedKey 为 key（sessionId 优先），这里双查兼容 legacy path key。
  const pendingConfirmation = useStore((st) => {
    const byPath = st.pendingSessionConfirmationsByPath;
    if (!byPath) return undefined;
    return (s.sessionId ? byPath[s.sessionId] : undefined) || byPath[s.path];
  });
  // 标识按决策类型匹配文案与颜色：提问最显眼（accent），批准次之（warning），其余中性。
  const actionBadge = useMemo(() => {
    if (!pendingConfirmation || pendingConfirmation.status !== 'pending') return null;
    const kind = pendingConfirmation.kind;
    if (kind === 'ask_user') {
      return { tone: 'ask', label: t('sidebar.actionBadge.askUser'), title: pendingConfirmation.title };
    }
    if (kind === 'tool_action_approval' || kind === 'computer_app_approval') {
      return { tone: 'approve', label: t('sidebar.actionBadge.approval'), title: pendingConfirmation.title };
    }
    return { tone: 'confirm', label: t('sidebar.actionBadge.confirm'), title: pendingConfirmation.title };
  }, [pendingConfirmation, t]);
  // 动态 import websocket，避免顶层静态依赖 websocket.ts 的模块加载副作用（见 InterludeBlock 同款注释）。
  const stopLoopForSession = useCallback(async (session: Session) => {
    const { getWebSocket } = await import('../services/websocket');
    const ws = getWebSocket();
    if (!ws || ws.readyState !== WebSocket.OPEN || !session.path) return;
    ws.send(JSON.stringify({ type: 'slash', text: '/loop stop', sessionPath: session.path, agentId: session.agentId || undefined }));
  }, []);

  const handleClick = useCallback(() => {
    if (editing) return;
    switchSession(s.path);
  }, [s.path, editing]);

  const handleArchive = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    onArchiveRequest(s);
  }, [onArchiveRequest, s]);

  const handlePin = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    if (isDeletedAgentSession && !isPinned) return;
    pinSession(s.path, !isPinned);
  }, [isDeletedAgentSession, s.path, isPinned]);

  const beginRename = useCallback(() => {
    if (isDeletedAgentSession) return;
    setEditValue(s.title || s.firstMessage || '');
    setEditing(true);
  }, [isDeletedAgentSession, s.title, s.firstMessage]);

  const commitRename = useCallback(() => {
    const trimmed = editValue.trim();
    setEditing(false);
    if (trimmed && trimmed !== (s.title || s.firstMessage || '')) {
      renameSession(s.path, trimmed);
    }
  }, [editValue, s.path, s.title, s.firstMessage]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      setEditing(false);
    }
  }, [commitRename]);

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setSummaryPreviewPosition(null);
    setMenuPosition({ x: e.clientX, y: e.clientY });
  }, []);

  // Auto-focus input when editing starts
  useEffect(() => {
    if (editing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editing]);

  // Meta line
  const parts: string[] = [];
  if (isDeletedAgentSession) parts.push(t('session.deletedAgent.meta'));
  if (s.agentName || s.agentId) parts.push(s.agentName || s.agentId!);
  if (s.cwd) {
    const dirName = s.cwd.split(/[/\\]/).filter(Boolean).pop();
    if (dirName) parts.push(dirName);
  }
  if (s.modified) parts.push(formatSessionDate(s.modified));
  const rcLabel = s.rcAttachment
    ? t('session.rcTakingOver', { platform: formatRcPlatform(s.rcAttachment.platform, t) })
    : null;
  const browserUrl = browserState?.url || null;
  const hasStatusSlot = !!browserUrl;
  // 状态点三态：失败（红点）> 运行中（旋转）> 完成未读（绿点）。
  // 红点最需要用户注意，优先级最高；切换加载不属于会话状态，本地切换又快，不画。
  const showStatusDot = hasFailed || isStreaming || hasUnreadOutput;
  const statusDotState = hasFailed ? 'failed' : isStreaming ? 'running' : 'unread';
  const isSingleLine = rowMode === 'single-line';
  const displayTitle = s.title || s.firstMessage || t('session.untitled');
  const metaText = parts.join(' · ');
  const itemTitle = [displayTitle, metaText, rcLabel].filter(Boolean).join('\n');
  const browserTitle = [
    browserUrl,
    browserState?.unavailableReason,
    t('browser.open'),
  ].filter(Boolean).join('\n');

  // 徽章左键 = 打开这个 session 的浏览器（冷状态先恢复，再让 viewer 切到它）。
  // 关闭是破坏性操作，收进右键菜单，避免误点中断 agent。
  const handleBrowserOpen = useCallback(async (e: React.MouseEvent | React.KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    try {
      await lingxiFetch('/api/browser/open-session', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sessionPath: s.path }),
      });
    } catch (err) {
      console.warn('[browser] open session failed:', err);
    }
    window.platform?.openBrowserViewer?.({ sessionPath: s.path });
  }, [s.path]);

  const handleBrowserKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    void handleBrowserOpen(e);
  }, [handleBrowserOpen]);

  const handleBrowserContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setBrowserMenuPosition({ x: e.clientX, y: e.clientY });
  }, []);

  const browserMenuItems = useMemo<ContextMenuItem[]>(() => ([{
    label: t('browser.closeForSession'),
    danger: true,
    action: () => onCloseBrowser(s.path),
  }]), [t, onCloseBrowser, s.path]);

  return (
    <>
      <button
        className={`${styles.sessionItem}${isSingleLine ? ` ${styles.sessionItemSingleLine}` : ''}${isActive ? ` ${styles.sessionItemActive}` : ''}${isDeletedAgentSession ? ` ${styles.sessionItemReadOnly}` : ''}${isChildRow ? ` ${styles.sessionItemChild}` : ''}${hasChildSessions ? ` ${styles.sessionItemParent}` : ''}`}
        data-session-path={s.path}
        data-children-folded={hasChildSessions ? (childrenFolded ? 'true' : 'false') : undefined}
        data-row-mode={rowMode}
        data-unread-output={hasUnreadOutput ? 'true' : 'false'}
        data-switch-pending={isPending ? 'true' : 'false'}
        title={itemTitle}
        draggable={draggable && !editing && !isDeletedAgentSession}
        onClick={handleClick}
        onContextMenu={handleContextMenu}
        onDragStart={draggable && !isDeletedAgentSession ? (event) => onDragStart?.(event, s) : undefined}
        onDragEnd={draggable && !isDeletedAgentSession ? onDragEnd : undefined}
      >
        <div className={styles.sessionItemHeader}>
          {s.agentId && (
            <AgentBadge
              agentId={s.agentId}
              agentName={s.agentName}
              agents={agents}
              interactive={hasChildSessions}
              folded={hasChildSessions && childrenFolded}
              foldedCount={hasChildSessions && childrenFolded ? (foldedChildCount || 0) : 0}
              onToggle={hasChildSessions ? () => onToggleChildren(s) : undefined}
              titleOverride={hasChildSessions
                ? (childrenFolded
                    ? ((foldedChildCount || 0) > 0
                        ? t('session.expandChildrenCount', { count: String(foldedChildCount) })
                        : t('session.expandChildren'))
                    : t('session.collapseChildren'))
                : undefined}
            />
          )}
          {showStatusDot && (
            <span
              className={styles.sessionStreamingDot}
              data-session-status-dot=""
              data-state={statusDotState}
              aria-hidden="true"
            />
          )}
          {loopStatus ? (
            <span
              className={styles.loopBadge}
              data-loop-state={loopStatus.status}
              title={loopStatus.status === 'running' ? '循环运行中' : '循环已暂停'}
              onPointerDown={(e) => e.stopPropagation()}
              onClick={(e) => e.stopPropagation()}
            >
              <span className={styles.loopBadgeDot} data-state={loopStatus.status} aria-hidden="true" />
              <span className={styles.loopBadgeStop} title="停止循环" onClick={() => stopLoopForSession(s)}>×</span>
            </span>
          ) : null}
          {editing ? (
            <input
              ref={inputRef}
              className={styles.sessionRenameInput}
              value={editValue}
              onChange={e => setEditValue(e.target.value)}
              onKeyDown={handleKeyDown}
              onBlur={commitRename}
              onClick={e => e.stopPropagation()}
            />
          ) : (
            <div className={styles.sessionItemTitle}>
              {displayTitle}
            </div>
          )}
          {actionBadge && !isActive && (
            <div
              className={styles.sessionActionBadge}
              data-tone={actionBadge.tone}
              title={actionBadge.title}
            >
              {actionBadge.label}
            </div>
          )}
          {hasStatusSlot && (
            <div className={styles.sessionStatusSlot}>
              {browserUrl && (
                <span
                  className={styles.sessionBrowserBadge}
                  title={browserTitle}
                  role="button"
                  tabIndex={0}
                  aria-label={t('browser.open')}
                  data-running={browserState?.running ? 'true' : 'false'}
                  data-resumable={browserState?.resumable ? 'true' : 'false'}
                  onClick={handleBrowserOpen}
                  onKeyDown={handleBrowserKeyDown}
                  onContextMenu={handleBrowserContextMenu}
                >
                  <BrowserStatusIcon />
                </span>
              )}
            </div>
          )}
          {isSingleLine && rcLabel && (
            <div className={styles.sessionRcBadgeInline}>
              {rcLabel}
            </div>
          )}
          <div className={styles.sessionItemActions} data-session-actions="">
            {!editing && (!isDeletedAgentSession || isPinned) && (
              <div className={styles.sessionPinBtn} title={t(isPinned ? 'session.unpin' : 'session.pin')} onClick={handlePin}>
                <PinIcon />
              </div>
            )}
            <div className={styles.sessionArchiveBtn} title={t('session.archive')} onClick={handleArchive}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="21 8 21 21 3 21 3 8" />
                <rect x="1" y="3" width="22" height="5" />
                <line x1="10" y1="12" x2="14" y2="12" />
              </svg>
            </div>
          </div>
        </div>

        {!isSingleLine && (
          <div className={styles.sessionItemMeta}>
            {metaText}
          </div>
        )}

        {!isSingleLine && rcLabel && (
          <div className={styles.sessionRcBadge}>
            {rcLabel}
          </div>
        )}

      </button>
      {menuPosition && (
        <SessionContextMenu
          session={s}
          isPinned={isPinned}
          position={menuPosition}
          onClose={() => setMenuPosition(null)}
          onRename={beginRename}
          onArchive={() => onArchiveRequest(s)}
          onShowSummary={(position) => setSummaryPreviewPosition(position)}
        />
      )}
      {summaryPreviewPosition && (
        <SessionSummaryPreviewCard
          session={s}
          position={summaryPreviewPosition}
          onClose={() => setSummaryPreviewPosition(null)}
        />
      )}
      {browserMenuPosition && (
        <ContextMenu
          items={browserMenuItems}
          position={browserMenuPosition}
          onClose={() => setBrowserMenuPosition(null)}
        />
      )}
    </>
  );
});

interface SessionSummaryResponse {
  hasSummary?: boolean;
  summary?: string | null;
  createdAt?: string | null;
  updatedAt?: string | null;
}

type SummaryState =
  | { status: 'loading'; text: null }
  | { status: 'ready'; text: string }
  | { status: 'empty'; text: null }
  | { status: 'error'; text: null };

const SessionContextMenu = memo(function SessionContextMenu({
  session,
  isPinned,
  position,
  onClose,
  onRename,
  onArchive,
  onShowSummary,
}: {
  session: Session;
  isPinned: boolean;
  position: { x: number; y: number };
  onClose: () => void;
  onRename: () => void;
  /** 归档入口：子对话存在时由上层弹选择框 */
  onArchive: (session: Session) => void;
  onShowSummary: (position: { x: number; y: number }) => void;
}) {
  const { t } = useI18n();
  const items = useMemo<ContextMenuItem[]>(() => {
    const menuItems: ContextMenuItem[] = [{
      label: t('session.summary.open'),
      disabled: session.hasSummary !== true,
      action: () => onShowSummary(position),
    }, {
      label: t('session.copyId'),
      disabled: typeof session.sessionId !== 'string' || !session.sessionId.trim(),
      action: () => {
        const sessionId = session.sessionId?.trim();
        if (!sessionId) {
          useStore.getState().addToast(t('session.copyIdUnavailable'), 'error', 5000);
          return;
        }
        if (!navigator.clipboard?.writeText) {
          useStore.getState().addToast(t('session.copyIdFailed'), 'error', 5000);
          return;
        }
        void navigator.clipboard.writeText(sessionId)
          .then(() => useStore.getState().addToast(t('session.copyIdDone'), 'info', 2500))
          .catch(() => useStore.getState().addToast(t('session.copyIdFailed'), 'error', 5000));
      },
    }];
    if (session.agentDeleted === true) {
      if (isPinned) {
        menuItems.push({
          label: t('session.unpin'),
          action: () => pinSession(session.path, false),
        });
      }
      menuItems.push({
        label: t('session.archive'),
        danger: true,
        action: () => onArchive(session),
      });
      return menuItems;
    }
    menuItems.push({
      label: t(isPinned ? 'session.unpin' : 'session.pin'),
      action: () => pinSession(session.path, !isPinned),
    });
    menuItems.push({
      label: t('session.rename'),
      action: onRename,
    });
    menuItems.push({
      label: t('session.archive'),
      danger: true,
      action: () => onArchive(session),
    });
    return menuItems;
  }, [isPinned, onArchive, onRename, onShowSummary, position, session.agentDeleted, session.hasSummary, session.path, session.sessionId, t]);

  return (
    <ContextMenu
      items={items}
      position={position}
      onClose={onClose}
    />
  );
});

const SessionSummaryPreviewCard = memo(function SessionSummaryPreviewCard({
  session,
  position,
  onClose,
}: {
  session: Session;
  position: { x: number; y: number };
  onClose: () => void;
}) {
  const { t } = useI18n();
  const cardRef = useRef<HTMLDivElement>(null);
  const [summaryState, setSummaryState] = useState<SummaryState>(
    session.hasSummary === true
      ? { status: 'loading', text: null }
      : { status: 'empty', text: null },
  );

  useEffect(() => {
    const card = cardRef.current;
    if (!card) return;
    const rect = card.getBoundingClientRect();
    let { x, y } = position;
    if (x + rect.width > window.innerWidth) x = Math.max(4, window.innerWidth - rect.width - 4);
    if (y + rect.height > window.innerHeight) y = Math.max(4, window.innerHeight - rect.height - 4);
    card.style.left = x + 'px';
    card.style.top = y + 'px';
  }, [position, summaryState]);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (cardRef.current?.contains(e.target as Node)) return;
      onClose();
    };
    const handleContextMenu = (e: MouseEvent) => {
      if (cardRef.current?.contains(e.target as Node)) return;
      onClose();
    };
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    const timer = setTimeout(() => {
      document.addEventListener('click', handleClick, true);
      document.addEventListener('contextmenu', handleContextMenu, true);
      document.addEventListener('keydown', handleKeyDown);
    });
    return () => {
      clearTimeout(timer);
      document.removeEventListener('click', handleClick, true);
      document.removeEventListener('contextmenu', handleContextMenu, true);
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [onClose]);

  useEffect(() => {
    if (session.hasSummary !== true) {
      setSummaryState({ status: 'empty', text: null });
      return;
    }

    let cancelled = false;
    setSummaryState({ status: 'loading', text: null });
    lingxiFetch(`/api/sessions/summary?path=${encodeURIComponent(session.path)}`)
      .then(res => res.json())
      .then((data: SessionSummaryResponse) => {
        if (cancelled) return;
        const summary = typeof data.summary === 'string' ? data.summary.trim() : '';
        if (data.hasSummary && summary) {
          setSummaryState({ status: 'ready', text: summary });
        } else {
          setSummaryState({ status: 'empty', text: null });
        }
      })
      .catch(() => {
        if (!cancelled) setSummaryState({ status: 'error', text: null });
      });

    return () => {
      cancelled = true;
    };
  }, [session.path, session.hasSummary]);

  const summaryHtml = useMemo(() => (
    summaryState.status === 'ready' ? renderMarkdown(summaryState.text) : ''
  ), [summaryState]);

  return createPortal(
    <div
      ref={cardRef}
      className={styles.sessionSummaryCard}
      style={{ left: position.x, top: position.y }}
      data-testid="session-summary-card"
      data-scrollable="true"
    >
      <div className={styles.sessionSummaryTitle}>{t('session.summary.title')}</div>
      <div className={styles.sessionSummaryBody}>
        {summaryState.status === 'ready' ? (
          <div dangerouslySetInnerHTML={{ __html: summaryHtml }} />
        ) : (
          <span className={styles.sessionSummaryPlaceholder}>
            {summaryState.status === 'loading'
              ? t('session.summary.loading')
              : summaryState.status === 'error'
                ? t('session.summary.loadFailed')
                : t('session.summary.empty')}
          </span>
        )}
      </div>
    </div>,
    document.body,
  );
});

function formatRcPlatform(platform: string, t: (key: string) => string) {
  const lower = (platform || '').toLowerCase();
  if (lower === 'tg' || lower === 'telegram') return t('bridge.platform.telegram');
  if (lower === 'feishu' || lower === 'fs') return t('bridge.platform.feishu');
  if (lower === 'wechat' || lower === 'wx') return t('bridge.platform.wechat');
  if (lower === 'qq') return t('bridge.platform.qq');
  return platform || t('bridge.platform.bridge');
}

// ── Agent Avatar Badge ──

const AgentBadge = memo(function AgentBadge({
  agentId,
  agentName,
  agents,
  interactive = false,
  folded = false,
  foldedCount = 0,
  onToggle,
  titleOverride,
}: {
  agentId: string;
  agentName: string | null;
  agents: Agent[];
  /** 主对话行：助手符号兼作子对话折叠开关（仅点击符号切换，不占整行） */
  interactive?: boolean;
  folded?: boolean;
  foldedCount?: number;
  onToggle?: () => void;
  titleOverride?: string;
}) {
  const info = resolveAgentDisplayInfo({
    id: agentId,
    agents,
    fallbackAgentName: agentName || agentId,
  });

  if (!interactive || !onToggle) {
    return (
      <AgentAvatar
        info={info}
        className={styles.sessionAgentBadge}
        title={agentName || agentId}
      />
    );
  }

  return (
    <span
      className={styles.sessionAgentToggle}
      data-agent-fold-toggle="true"
      data-folded={folded ? 'true' : 'false'}
      role="button"
      tabIndex={0}
      aria-expanded={!folded}
      aria-label={titleOverride || agentName || agentId}
      title={titleOverride || agentName || agentId}
      onClick={(e) => {
        e.stopPropagation();
        onToggle();
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          e.stopPropagation();
          onToggle();
        }
      }}
    >
      <AgentAvatar info={info} className={styles.sessionAgentBadge} />
      {/* 折叠态：符号本身的变化——右上角出现被收起的子对话数徽标 */}
      {folded && (
        <span className={styles.sessionAgentFoldCount} data-agent-fold-count="true">{foldedCount > 0 ? foldedCount : '·'}</span>
      )}
    </span>
  );
});

/**
 * 归档带子对话的主对话：三选一。
 * 一起归档 = 递归归档全部子对话；仅归档主对话 = 先把子对话释放到列表顶层；取消。
 */
function SessionArchiveChoiceDialog({
  session,
  childCount,
  onClose,
}: {
  session: Session;
  childCount: number;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);
  const run = (mode: 'archive_children' | 'detach_children') => {
    setBusy(true);
    void archiveSession(session.path, mode).finally(() => {
      setBusy(false);
      onClose();
    });
  };
  return (
    <Overlay
      open
      scope="window"
      onClose={busy ? () => {} : onClose}
      backdrop="dim"
      zIndex={320}
      className={styles.archiveChoiceDialog}
      contentProps={{ role: 'dialog', 'aria-modal': true, 'data-testid': 'session-archive-choice-dialog' }}
    >
      <h2 className={styles.archiveChoiceTitle}>{t('session.archiveChildrenTitle')}</h2>
      <p className={styles.archiveChoiceBody}>
        {t('session.archiveChildrenBody', { count: String(childCount) })}
      </p>
      <div className={styles.archiveChoiceActions}>
        <Button variant="secondary" onClick={onClose} disabled={busy} className={styles.archiveChoiceAction}>
          {t('common.cancel')}
        </Button>
        <Button variant="secondary" onClick={() => run('detach_children')} disabled={busy} className={styles.archiveChoiceAction}>
          {t('session.archiveDetachChildren')}
        </Button>
        <Button variant="danger" onClick={() => run('archive_children')} loading={busy} className={styles.archiveChoiceAction}>
          {t('session.archiveWithChildren')}
        </Button>
      </div>
    </Overlay>
  );
}
