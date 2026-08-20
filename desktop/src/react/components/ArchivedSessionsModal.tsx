import { useCallback, useEffect, useState } from 'react';
import { useI18n } from '../hooks/use-i18n';
import { Overlay } from '../ui';
import {
  listArchivedSessions,
  restoreSession,
  deleteArchivedSession,
  cleanupArchivedSessions,
  showSidebarToast,
  type ArchivedSession,
} from '../stores/session-actions';
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
              <button
                className={styles.deleteSelectedBtn}
                onClick={handleDeleteSelected}
                disabled={selected.size === 0}
              >
                {t('session.archived.deleteSelected', { count: selected.size })}
              </button>
            </div>
            <div className={styles.list}>
              {loading ? (
                <div className={styles.loading}>{t('common.loading')}</div>
              ) : list.length === 0 ? (
                <div className={styles.empty}>{t('session.archived.empty')}</div>
              ) : (
                list.map((item) => (
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
                ))
              )}
            </div>
          </div>
        </div>
    </Overlay>
  );
}
