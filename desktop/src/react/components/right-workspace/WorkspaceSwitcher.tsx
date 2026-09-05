/**
 * WorkspaceSwitcher — 工作台标题下拉切换器
 *
 * 「对话文件 / 工作台 / 项目技能」tabs 上方的工作台标题本身是触发钮：
 * 点开列出可切换的工作台（挂载工作台 + 使用历史/主目录目录，与欢迎页选择器
 * 同一数据口径与切换语义，见 utils/workspace-switch），点条目一键切换；
 * 本地 folder picker 可用时保留「选择其他文件夹」入口。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../../stores';
import { applyStudioWorkspace, loadStudioWorkspaces } from '../../stores/desk-actions';
import {
  applyWorkspaceHistoryFolder,
  browseAndApplyLocalWorkspace,
  collectAgentHomeFolders,
  findAgentByHomeFolder,
  removeStudioWorkspaceWithDisposal,
  removeWorkspaceHistoryEntry,
} from '../../utils/workspace-switch';
import { isSameWorkspacePath } from '../../utils/agent-workspace';
import { buildWorkspacePickerItems, normalizeWorkspacePath, workspaceDisplayName } from '../../../../../shared/workspace-history.ts';
import type { StudioWorkspace } from '../../types';
import styles from './RightWorkspacePanel.module.css';

function FolderIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
    </svg>
  );
}

export function WorkspaceSwitcher() {
  const deskBasePath = useStore(s => s.deskBasePath);
  const deskWorkspaceMountId = useStore(s => s.deskWorkspaceMountId);
  const deskWorkspaceLabel = useStore(s => s.deskWorkspaceLabel);
  const selectedFolder = useStore(s => s.selectedFolder);
  const homeFolder = useStore(s => s.homeFolder);
  const studioWorkspaces = useStore(s => s.studioWorkspaces);
  const cwdHistory = useStore(s => s.cwdHistory);
  const agents = useStore(s => s.agents);
  const currentAgentId = useStore(s => s.currentAgentId);
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const t = window.t ?? ((p: string) => p);

  const title = deskWorkspaceMountId
    ? (deskWorkspaceLabel || deskWorkspaceMountId)
    : workspaceDisplayName(deskBasePath || selectedFolder || homeFolder, t('desk.title'));
  const titlePath = deskWorkspaceMountId ? title : (deskBasePath || selectedFolder || homeFolder || undefined);

  // 与欢迎页选择器同一数据口径：默认工作台不单列（同一目录经主目录/历史条目进入，
  // 走 mount 形态）；同一目录既是挂载、又以历史条目出现时只保留挂载行。
  const agentHomeFolders = useMemo(() => collectAgentHomeFolders(agents), [agents]);
  const { visibleStudioWorkspaces, primaryItems } = useMemo(() => {
    const mounts = studioWorkspaces.filter(workspace => !workspace.isDefault);
    const mountedRoots = mounts
      .map(workspace => workspace.nativeRootPath)
      .filter((root): root is string => typeof root === 'string' && !!root);
    const folders = buildWorkspacePickerItems({
      selectedFolder,
      homeFolder,
      cwdHistory: [...agentHomeFolders, ...cwdHistory],
    }).filter(p => !mountedRoots.some(root => isSameWorkspacePath(root, p)));
    return { visibleStudioWorkspaces: mounts, primaryItems: folders };
  }, [agentHomeFolders, cwdHistory, homeFolder, selectedFolder, studioWorkspaces]);

  // 打开即刷新挂载列表：欢迎页之外的新入口也要拿到最新工作台
  useEffect(() => {
    if (open) void loadStudioWorkspaces();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    const timer = setTimeout(() => document.addEventListener('click', close, true), 0);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('click', close, true);
    };
  }, [open]);

  const isFolderActive = useCallback((folder: string) => {
    if (deskWorkspaceMountId) return false;
    return isSameWorkspacePath(deskBasePath, folder) || isSameWorkspacePath(selectedFolder, folder);
  }, [deskBasePath, deskWorkspaceMountId, selectedFolder]);

  const handleSelectWorkspace = useCallback((workspace: StudioWorkspace) => {
    setOpen(false);
    // 已在目标工作台：只收起菜单，不重置会话草稿
    if (deskWorkspaceMountId === workspace.mountId) return;
    void applyStudioWorkspace(workspace);
  }, [deskWorkspaceMountId]);

  const handleSelectFolder = useCallback((folder: string) => {
    setOpen(false);
    const agent = findAgentByHomeFolder(agents, folder);
    if (agent) {
      const st = useStore.getState();
      const activeAgentId = st.selectedAgentId || st.currentAgentId;
      // 已在该 Agent 的默认工作台：只收起菜单，不重置会话草稿
      if (st.deskWorkspaceMountId === 'default' && agent.id === activeAgentId) return;
    } else if (isFolderActive(folder)) {
      return;
    }
    void applyWorkspaceHistoryFolder(agents, currentAgentId, folder);
  }, [agents, currentAgentId, isFolderActive]);

  const handleBrowse = useCallback(() => {
    setOpen(false);
    void browseAndApplyLocalWorkspace();
  }, []);

  // 移除挂载工作台：与欢迎页选择器同语义（名下对话直接归档后移除），移除即收起菜单。
  const handleRemoveWorkspace = useCallback((mountId: string) => {
    setOpen(false);
    void removeStudioWorkspaceWithDisposal(mountId);
  }, []);

  // 从使用历史移除一条目录记录：轻操作，菜单保持展开，行即时消失。
  const handleRemoveHistory = useCallback((folder: string) => {
    void removeWorkspaceHistoryEntry(folder);
  }, []);

  const removableHistory = useMemo(
    () => new Set(cwdHistory.map(normalizeWorkspacePath).filter(Boolean)),
    [cwdHistory],
  );

  const canBrowse = typeof window.platform?.selectFolder === 'function';
  const hasEntries = visibleStudioWorkspaces.length > 0 || primaryItems.length > 0 || canBrowse;

  return (
    <div className={styles.workspaceSwitcherWrap} ref={wrapRef}>
      <button
        type="button"
        className={styles.workspaceSwitcher}
        data-right-workspace-switcher=""
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={t('input.selectWorkspace')}
        title={titlePath}
        onClick={() => setOpen(prev => !prev)}
      >
        <span className={styles.workspaceTitle}>{title}</span>
        <svg
          className={`${styles.workspaceSwitcherChevron}${open ? ` ${styles.workspaceSwitcherChevronOpen}` : ''}`}
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <polyline points="6 9 12 15 18 9"></polyline>
        </svg>
      </button>
      {open && hasEntries && (
        <div
          className={styles.workspaceSwitcherMenu}
          data-right-workspace-switcher-menu=""
          role="listbox"
          aria-label={t('input.selectWorkspace')}
        >
          {visibleStudioWorkspaces.map(workspace => {
            const active = deskWorkspaceMountId === workspace.mountId;
            return (
              <div
                key={`studio:${workspace.mountId}`}
                className={`${styles.workspaceSwitcherItem}${active ? ` ${styles.workspaceSwitcherItemActive}` : ''}`}
                role="option"
                aria-selected={active}
                title={workspace.nativeRootPath || workspace.label}
                onClick={() => handleSelectWorkspace(workspace)}
              >
                <span className={styles.workspaceSwitcherItemIcon}><FolderIcon /></span>
                <span className={styles.workspaceSwitcherItemName}>{workspace.label}</span>
                <button
                  type="button"
                  className={styles.workspaceSwitcherRemove}
                  onClick={(e) => {
                    e.stopPropagation();
                    handleRemoveWorkspace(workspace.mountId);
                  }}
                  title={t('input.removeStudioWorkspace')}
                  aria-label={t('input.removeStudioWorkspace')}
                >
                  x
                </button>
              </div>
            );
          })}
          {primaryItems.map(p => {
            const active = isFolderActive(p);
            const removable = removableHistory.has(normalizeWorkspacePath(p));
            return (
              <div
                key={p}
                className={`${styles.workspaceSwitcherItem}${active ? ` ${styles.workspaceSwitcherItemActive}` : ''}`}
                role="option"
                aria-selected={active}
                title={p}
                onClick={() => handleSelectFolder(p)}
              >
                <span className={styles.workspaceSwitcherItemIcon}><FolderIcon /></span>
                <span className={styles.workspaceSwitcherItemName}>{workspaceDisplayName(p) || p}</span>
                {removable && (
                  <button
                    type="button"
                    className={styles.workspaceSwitcherRemove}
                    onClick={(e) => {
                      e.stopPropagation();
                      handleRemoveHistory(p);
                    }}
                    title={t('input.removeRecentWorkspace')}
                    aria-label={t('input.removeRecentWorkspace')}
                  >
                    x
                  </button>
                )}
              </div>
            );
          })}
          {canBrowse && (
            <>
              <div className={styles.workspaceSwitcherDivider} />
              <div
                className={styles.workspaceSwitcherItem}
                role="option"
                aria-selected={false}
                onClick={handleBrowse}
              >
                <span className={styles.workspaceSwitcherItemIcon}>
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                    <line x1="12" y1="11" x2="12" y2="17"></line>
                    <line x1="9" y1="14" x2="15" y2="14"></line>
                  </svg>
                </span>
                <span className={styles.workspaceSwitcherItemName}>{t('input.selectOtherFolder')}</span>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
