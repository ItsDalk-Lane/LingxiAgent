import { useCallback, useEffect, useMemo, useState } from 'react';
import { useI18n } from '../hooks/use-i18n';
import { useStore } from '../stores';
import { Overlay } from '../ui';
import {
  listArchivedSessions,
  restoreSession,
  deleteArchivedSession,
  cleanupArchivedSessions,
  showSidebarToast,
  type ArchivedSession,
} from '../stores/session-actions';
import { isSameWorkspacePath } from '../utils/agent-workspace';
import styles from './ArchivedSessionsModal.module.css';

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function formatAgo(iso: string, t: (k: string, v?: Record<string, string | number>) => string): string {
  const ms = Date.now() - new Date(iso).getTime();
  const days = Math.floor(ms / 86400_000);
  if (days < 1) return t('time.today');
  if (days === 1) return t('time.yesterday');
  return t('session.archived.daysAgo', { days });
}

function pathBasename(p: string): string {
  const trimmed = p.replace(/[\\/]+$/, '');
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return trimmed.slice(idx + 1) || trimmed;
}

interface ArchiveGroup {
  key: string;
  mountId: string | null;
  cwd: string | null;
  /** 工作台是否仍存在（不存在 → 「该工作目录已移除」徽标） */
  workspaceExists: boolean;
  title: string;
  items: ArchivedSession[];
}

interface Props {
  open: boolean;
  onClose: () => void;
  zIndex?: number;
}

export function ArchivedSessionsModal({ open, onClose, zIndex = 1000 }: Props) {
  const { t } = useI18n();
  const [list, setList] = useState<ArchivedSession[]>([]);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  // 分组折叠态（key 稳定：mount:/path:/ungrouped），刷新列表后保留
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());
  const studioWorkspaces = useStore(s => s.studioWorkspaces);
  const defaultWorkspaceRootPath = useStore(s => s.defaultWorkspaceRootPath);

  const toggleGroupCollapse = useCallback((key: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    setList(await listArchivedSessions());
    setSelected(new Set());
    setLoading(false);
  }, []);

  useEffect(() => {
    if (open) refresh();
  }, [open, refresh]);

  const totalSize = list.reduce((s, x) => s + x.sizeBytes, 0);
  const allSelected = list.length > 0 && selected.size === list.length;

  // ── 按工作台分组（用户需求：知道哪些归档属于哪个工作目录）──
  // mount 形态按 mountId 分组；无 mount 的老会话按 cwd 目录分组；两者皆无 → 未归属。
  const groups = useMemo<ArchiveGroup[]>(() => {
    const knownMountRoots = studioWorkspaces
      .map(workspace => workspace.nativeRootPath || null)
      .filter((p): p is string => !!p);
    const mountExists = (mountId: string) => mountId === 'default'
      || studioWorkspaces.some(workspace => workspace.mountId === mountId);
    const pathExists = (cwd: string) => (
      (defaultWorkspaceRootPath && isSameWorkspacePath(cwd, defaultWorkspaceRootPath))
      || knownMountRoots.some(root => isSameWorkspacePath(cwd, root))
    );

    const byKey = new Map<string, ArchiveGroup>();
    for (const item of list) {
      const mountId = item.workspaceMountId?.trim() || null;
      const cwd = item.cwd?.trim() || null;
      const key = mountId ? `mount:${mountId}` : (cwd ? `path:${cwd}` : 'ungrouped');
      let group = byKey.get(key);
      if (!group) {
        const workspaceExists = mountId
          ? mountExists(mountId)
          : (cwd ? pathExists(cwd) : false);
        let title: string;
        if (mountId === 'default') {
          // 与主界面同规则：配置目录名，未配置才 Default（cwd 是该 mount 的解析根）。
          title = (cwd && pathBasename(cwd)) || 'Default';
        } else if (mountId) {
          title = item.workspaceLabel?.trim() || mountId;
        } else if (cwd) {
          title = pathBasename(cwd);
        } else {
          title = t('session.archived.group.ungrouped');
        }
        group = { key, mountId, cwd, workspaceExists, title, items: [] };
        byKey.set(key, group);
      }
      group.items.push(item);
    }
    return [...byKey.values()];
  }, [list, studioWorkspaces, defaultWorkspaceRootPath, t]);

  const toggleSelected = (item: ArchivedSession) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(item.path)) next.delete(item.path);
      else next.add(item.path);
      return next;
    });
  };

  const toggleAll = () => {
    setSelected(allSelected ? new Set() : new Set(list.map((x) => x.path)));
  };

  const toggleGroup = (group: ArchiveGroup) => {
    setSelected((prev) => {
      const next = new Set(prev);
      const allIn = group.items.every(item => next.has(item.path));
      for (const item of group.items) {
        if (allIn) next.delete(item.path);
        else next.add(item.path);
      }
      return next;
    });
  };

  const handleRestore = async (item: ArchivedSession) => {
    if (!window.confirm(t('session.archived.restoreConfirm'))) return;
    const r = await restoreSession(item);
    if (r.status === 'conflict') {
      showSidebarToast(t('session.archived.restoreConflict'));
      return;
    }
    if (r.status === 'error') {
      showSidebarToast(t('session.archived.restoreFailed'));
      return;
    }
    await refresh();
  };

  const handleDelete = async (item: ArchivedSession) => {
    if (!window.confirm(t('session.archived.deleteConfirm'))) return;
    const ok = await deleteArchivedSession(item);
    if (ok) await refresh();
    else showSidebarToast(t('session.archived.deleteFailed'));
  };

  const handleDeleteSelected = async () => {
    const targets = list.filter((x) => selected.has(x.path));
    if (targets.length === 0) return;
    const size = targets.reduce((s, x) => s + x.sizeBytes, 0);
    const msg = t('session.archived.deleteSelectedConfirm', {
      count: targets.length,
      size: formatBytes(size),
    });
    if (!window.confirm(msg)) return;
    let deleted = 0;
    for (const item of targets) {
      if (await deleteArchivedSession(item)) deleted += 1;
    }
    if (deleted < targets.length) {
      showSidebarToast(t('session.archived.deleteSelectedPartial', { deleted, total: targets.length }));
    } else {
      showSidebarToast(t('session.archived.deleteSelectedDone', { count: deleted }));
    }
    await refresh();
  };

  // 批量恢复共用：不跳转会话，只统计恢复/冲突数并提示（单条恢复仍走 handleRestore）
  const handleRestoreBatch = async (targets: ArchivedSession[], successToast: string, params: Record<string, string | number>) => {
    let restored = 0;
    let conflicts = 0;
    for (const item of targets) {
      const r = await restoreSession(item, { switchTo: false });
      if (r.status === 'ok') restored += 1;
      else if (r.status === 'conflict') conflicts += 1;
    }
    if (restored === targets.length) {
      showSidebarToast(t(successToast, params));
    } else if (restored > 0) {
      showSidebarToast(t('session.archived.restoreSelectedPartial', { restored, total: targets.length }));
    } else if (conflicts > 0) {
      showSidebarToast(t('session.archived.restoreConflict'));
    } else {
      showSidebarToast(t('session.archived.restoreFailed'));
    }
    await refresh();
  };

  const handleRestoreSelected = async () => {
    const targets = list.filter((x) => selected.has(x.path));
    if (targets.length === 0) return;
    if (!window.confirm(t('session.archived.restoreSelectedConfirm', { count: targets.length }))) return;
    await handleRestoreBatch(targets, 'session.archived.restoreSelectedDone', { count: targets.length });
  };

  // 整组删除：按工作台分组直接永久删除该组全部归档记录
  const handleDeleteGroup = async (group: ArchiveGroup) => {
    const size = group.items.reduce((s, x) => s + x.sizeBytes, 0);
    const msg = t('session.archived.deleteGroupConfirm', {
      name: group.title,
      count: group.items.length,
      size: formatBytes(size),
    });
    if (!window.confirm(msg)) return;
    let deleted = 0;
    for (const item of group.items) {
      if (await deleteArchivedSession(item)) deleted += 1;
    }
    if (deleted < group.items.length) {
      showSidebarToast(t('session.archived.deleteSelectedPartial', { deleted, total: group.items.length }));
    } else {
      showSidebarToast(t('session.archived.deleteGroupDone', { count: deleted }));
    }
    await refresh();
  };

  // 整组恢复：按工作台分组把该组全部归档记录恢复到会话列表
  const handleRestoreGroup = async (group: ArchiveGroup) => {
    if (!window.confirm(t('session.archived.restoreGroupConfirm', { name: group.title, count: group.items.length }))) return;
    await handleRestoreBatch(group.items, 'session.archived.restoreSelectedDone', { count: group.items.length });
  };

  const handleCleanup = async (days: 30 | 90) => {
    const toDelete = list.filter(
      (x) => Date.now() - new Date(x.archivedAt).getTime() > days * 86400_000,
    );
    if (toDelete.length === 0) {
      showSidebarToast(t('session.archived.cleanupNoMatch'));
      return;
    }
    const size = toDelete.reduce((s, x) => s + x.sizeBytes, 0);
    const msg = t('session.archived.cleanupConfirm', {
      count: toDelete.length,
      size: formatBytes(size),
    });
    if (!window.confirm(msg)) return;
    const { deleted } = await cleanupArchivedSessions(days);
    showSidebarToast(t('session.archived.cleanupDone', { count: deleted }));
    await refresh();
  };

  return (
    <Overlay
      scope="inline"
      open={open}
      onClose={onClose}
      backdrop="blur"
      zIndex={zIndex}
      className={styles.modal}
      disableContainerAnimation
    >
        <div className={styles.header}>
          <h2 className={styles.title}>{t('session.archived.title')}</h2>
          <button className={styles.closeBtn} onClick={onClose} aria-label="Close">
            ×
          </button>
        </div>

        <div className={styles.content}>
          <div className={styles.summaryCard}>
            <span className={styles.summaryText}>
              {t('session.archived.stats', {
                count: list.length,
                size: formatBytes(totalSize),
              })}
            </span>
            <div className={styles.cleanupBtns}>
              <button onClick={() => handleCleanup(30)}>
                {t('session.archived.cleanup30')}
              </button>
              <button onClick={() => handleCleanup(90)}>
                {t('session.archived.cleanup90')}
              </button>
            </div>
          </div>

          <div className={styles.listCard}>
            <div className={styles.listToolbar}>
              <label className={styles.selectAll}>
                <input
                  type="checkbox"
                  checked={allSelected}
                  onChange={toggleAll}
                  disabled={list.length === 0}
                />
                <span>{t('session.archived.selectAll')}</span>
              </label>
              <div className={styles.batchBtns}>
                <button
                  className={styles.restoreSelectedBtn}
                  onClick={handleRestoreSelected}
                  disabled={selected.size === 0}
                >
                  {t('session.archived.restoreSelected', { count: selected.size })}
                </button>
                <button
                  className={styles.deleteSelectedBtn}
                  onClick={handleDeleteSelected}
                  disabled={selected.size === 0}
                >
                  {t('session.archived.deleteSelected', { count: selected.size })}
                </button>
              </div>
            </div>
            <div className={styles.list}>
              {loading ? (
                <div className={styles.loading}>{t('common.loading')}</div>
              ) : list.length === 0 ? (
                <div className={styles.empty}>{t('session.archived.empty')}</div>
              ) : (
                groups.map((group) => {
                  const groupSize = group.items.reduce((s, x) => s + x.sizeBytes, 0);
                  const allInGroup = group.items.length > 0
                    && group.items.every(item => selected.has(item.path));
                  const collapsed = collapsedGroups.has(group.key);
                  return (
                    <div key={group.key} className={styles.group} data-archive-group={group.key}>
                      {/* 分组头可折叠整组：点击头部切换，行内勾选/删除按钮各自 stopPropagation */}
                      <div
                        className={styles.groupHeader}
                        data-group-header={group.key}
                        data-collapsed={collapsed ? 'true' : 'false'}
                        role="button"
                        tabIndex={0}
                        aria-expanded={!collapsed}
                        onClick={() => toggleGroupCollapse(group.key)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' || e.key === ' ') {
                            e.preventDefault();
                            toggleGroupCollapse(group.key);
                          }
                        }}
                      >
                        <span className={`${styles.chevron}${collapsed ? '' : ` ${styles.chevronOpen}`}`} aria-hidden="true">
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                            <polyline points="9 18 15 12 9 6"></polyline>
                          </svg>
                        </span>
                        <input
                          type="checkbox"
                          className={styles.groupCheck}
                          checked={allInGroup}
                          onChange={() => toggleGroup(group)}
                          onClick={(e) => e.stopPropagation()}
                          aria-label={group.title}
                        />
                        <span className={styles.groupName} title={group.cwd || group.mountId || undefined}>
                          {group.title}
                        </span>
                        {/* 徽标只给「有工作台身份但已解析不到」的分组；未归属组从未有过工作台，不标 */}
                        {(group.mountId || group.cwd) && !group.workspaceExists && (
                          <span className={styles.removedBadge}>
                            {t('session.archived.group.workspaceRemoved')}
                          </span>
                        )}
                        {group.cwd && (
                          <span className={styles.groupPath} title={group.cwd}>{group.cwd}</span>
                        )}
                        <span className={styles.groupMeta}>
                          {t('session.archived.group.meta', { count: group.items.length, size: formatBytes(groupSize) })}
                        </span>
                        <button
                          className={styles.groupRestoreBtn}
                          onClick={(e) => {
                            e.stopPropagation();
                            void handleRestoreGroup(group);
                          }}
                        >
                          {t('session.archived.restoreGroup')}
                        </button>
                        <button
                          className={styles.groupDeleteBtn}
                          onClick={(e) => {
                            e.stopPropagation();
                            void handleDeleteGroup(group);
                          }}
                        >
                          {t('session.archived.deleteGroup')}
                        </button>
                      </div>
                      {!collapsed && group.items.map((item) => (
                        <div key={item.path} className={styles.row}>
                          <input
                            type="checkbox"
                            className={styles.rowCheck}
                            checked={selected.has(item.path)}
                            onChange={() => toggleSelected(item)}
                            aria-label={item.title || item.firstMessage || t('session.untitled')}
                          />
                          <div className={styles.rowMain}>
                            <div className={styles.rowTitle}>
                              {item.title || item.firstMessage || t('session.untitled')}
                            </div>
                            <div className={styles.rowMeta}>
                              {item.agentName} · {formatAgo(item.archivedAt, t)} ·{' '}
                              {formatBytes(item.sizeBytes)}
                            </div>
                          </div>
                          <div className={styles.rowActions}>
                            <button
                              title={t('session.archived.restore')}
                              onClick={() => handleRestore(item)}
                            >
                              {t('session.archived.restore')}
                            </button>
                            <button
                              title={t('session.archived.deleteForever')}
                              onClick={() => handleDelete(item)}
                            >
                              {t('session.archived.deleteForever')}
                            </button>
                          </div>
                        </div>
                      ))}
                    </div>
                  );
                })
              )}
            </div>
          </div>
        </div>
    </Overlay>
  );
}
