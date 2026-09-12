/**
 * GitChangesModal — 变更文件列表弹窗（环境信息卡·变更行入口）
 *
 * 每行 = 文件名（超长省略号截断）+ 该文件 +N -N 增删 + 三个行级操作：
 *   暂存  该文件改动收进储藏栈（git stash push -u -- <path>）
 *   取出  把该文件从最新一条含它的储藏里取回工作区（不删储藏条目）
 *   回退  丢弃该文件未提交改动，恢复成 HEAD（未跟踪文件不提供回退）
 * 顶部为整体操作：暂存全部 / 取出全部（弹出最新一条储藏）/ 回退全部（破坏性，需确认）。
 * 点击文件名区域展开行级 diff（懒加载、会话内缓存）。
 */
import { useCallback, useEffect, useState } from 'react';
import { ConfirmDialog, Overlay, Tooltip } from '../../ui';
import { useStore } from '../../stores';
import {
  fetchGitFileDiff,
  fetchGitStashes,
  gitDiscard,
  gitStash,
  gitUnstash,
  type GitActionResult,
  type GitFileChange,
  type GitFileDiff,
  type GitStashEntry,
} from '../../utils/git-env-api';
import { parseUnifiedPatch } from '../../utils/unified-diff';
import styles from './GitChangesModal.module.css';

const MAX_RENDER_LINES = 1500;

type DiffCache = Record<string, GitFileDiff | 'error'>;

/** 单文件回退不可用：未跟踪文件没有 HEAD 版本可回 */
function canDiscard(file: GitFileChange): boolean {
  return file.state !== 'untracked';
}

/** 该文件是否躺在某条储藏里（决定「取出」是否可用） */
function stashForPath(stashes: GitStashEntry[], path: string): GitStashEntry | null {
  return stashes.find(entry => entry.tracked.includes(path) || entry.untracked.includes(path)) ?? null;
}

function fmt(n: number): string {
  return n.toLocaleString('en-US');
}

interface GitChangesModalProps {
  open: boolean;
  onClose: () => void;
  dir: string;
  files: GitFileChange[];
  agentId: string | null;
  /** 操作后让整卡重新取数 */
  refresh: () => Promise<unknown>;
}

export function GitChangesModal({ open, onClose, dir, files, agentId, refresh }: GitChangesModalProps) {
  const t = window.t ?? ((p: string) => p);
  const addToast = useStore(s => s.addToast);
  const [expandedPath, setExpandedPath] = useState<string | null>(null);
  const [loadingPath, setLoadingPath] = useState<string | null>(null);
  const [diffs, setDiffs] = useState<DiffCache>({});
  const [stashes, setStashes] = useState<GitStashEntry[]>([]);
  const [busyPath, setBusyPath] = useState<string | null>(null);
  const [bulkBusy, setBulkBusy] = useState<null | 'stash' | 'unstash' | 'discard'>(null);
  const [confirmDiscard, setConfirmDiscard] = useState<string[] | null>(null);

  // 关闭重置展开态；diff 缓存保留到目录变化（组件由卡片持有，dir 变化即重挂载链路之外）
  useEffect(() => {
    if (!open) {
      setExpandedPath(null);
      setLoadingPath(null);
      setBusyPath(null);
      setBulkBusy(null);
      setConfirmDiscard(null);
    }
  }, [open]);

  useEffect(() => {
    setDiffs({});
    setExpandedPath(null);
  }, [dir]);

  const loadStashes = useCallback(async () => {
    try {
      const result = await fetchGitStashes(dir, agentId);
      setStashes(result.stashes ?? []);
    } catch {
      setStashes([]);
    }
  }, [dir, agentId]);

  // 储藏栈按需拉取：只在弹窗打开时取，不给卡片刷新加固定开销
  useEffect(() => {
    if (!open) return;
    void loadStashes();
  }, [open, loadStashes]);

  const toggleFile = async (file: GitFileChange) => {
    if (expandedPath === file.path) {
      setExpandedPath(null);
      return;
    }
    setExpandedPath(file.path);
    if (diffs[file.path] == null) {
      setLoadingPath(file.path);
      try {
        const diff = await fetchGitFileDiff(dir, file.path);
        setDiffs(prev => ({ ...prev, [file.path]: diff }));
      } catch {
        setDiffs(prev => ({ ...prev, [file.path]: 'error' }));
      } finally {
        setLoadingPath(null);
      }
    }
  };

  const describeFailure = (result: GitActionResult, fallback: string): string => {
    if (result.code === 'nothing_to_stash') return t('gitEnv.nothingToStash');
    if (result.code === 'no_stash') return t('gitEnv.unstashNoStash');
    if (result.code === 'not_in_stash') return t('gitEnv.unstashNotInStash');
    if (result.code === 'path_dirty') return t('gitEnv.unstashPathDirty');
    if (result.code === 'unstash_conflict') return t('gitEnv.unstashConflict');
    if (result.code === 'nothing_to_discard') return t('gitEnv.discardNothing');
    return result.message || result.error || fallback;
  };

  const handleStashFile = async (file: GitFileChange) => {
    if (busyPath) return;
    setBusyPath(file.path);
    try {
      const result = await gitStash(dir, { paths: [file.path], agentId });
      if (!result.httpOk || !result.ok) {
        addToast?.(describeFailure(result, t('gitEnv.stashFileFailed')), 'error');
        return;
      }
      addToast?.(t('gitEnv.stashFileDone', { name: file.path }), 'success');
      setDiffs(prev => {
        const next = { ...prev };
        delete next[file.path];
        return next;
      });
      await Promise.all([refresh(), loadStashes()]);
    } finally {
      setBusyPath(null);
    }
  };

  const handleUnstashFile = async (file: GitFileChange) => {
    if (busyPath) return;
    setBusyPath(file.path);
    try {
      const result = await gitUnstash(dir, { path: file.path, agentId });
      if (!result.httpOk || !result.ok) {
        addToast?.(describeFailure(result, t('gitEnv.unstashFileFailed')), 'error');
        return;
      }
      addToast?.(t('gitEnv.unstashFileDone', { name: file.path }), 'success');
      setDiffs(prev => {
        const next = { ...prev };
        delete next[file.path];
        return next;
      });
      await Promise.all([refresh(), loadStashes()]);
    } finally {
      setBusyPath(null);
    }
  };

  const runDiscard = async (paths: string[] | null) => {
    const result = await gitDiscard(dir, { ...(paths ? { paths } : {}), agentId });
    if (!result.httpOk || !result.ok) {
      addToast?.(describeFailure(result, t('gitEnv.discardFailed')), 'error');
      return;
    }
    addToast?.(paths
      ? t('gitEnv.discardFileDone', { name: paths[0] })
      : t('gitEnv.discardAllDone'), 'success');
    setDiffs(prev => {
      const next = { ...prev };
      for (const path of paths ?? Object.keys(next)) delete next[path];
      return next;
    });
    await refresh();
  };

  /** 回退是破坏性操作：单个与整体都先走确认弹窗（`[]` = 整个仓库） */
  const handleDiscardFile = (file: GitFileChange) => {
    if (busyPath || bulkBusy) return;
    setConfirmDiscard([file.path]);
  };

  const handleBulkStash = async () => {
    if (bulkBusy) return;
    setBulkBusy('stash');
    try {
      const result = await gitStash(dir, { message: t('gitEnv.stashAllLabel'), agentId });
      if (!result.httpOk || !result.ok) {
        addToast?.(describeFailure(result, t('gitEnv.stashFileFailed')), 'error');
        return;
      }
      addToast?.(t('gitEnv.stashAllDone'), 'success');
      setDiffs({});
      await Promise.all([refresh(), loadStashes()]);
    } finally {
      setBulkBusy(null);
    }
  };

  const handleBulkUnstash = async () => {
    if (bulkBusy) return;
    setBulkBusy('unstash');
    try {
      const result = await gitUnstash(dir, { agentId });
      if (!result.httpOk || !result.ok) {
        addToast?.(describeFailure(result, t('gitEnv.unstashFileFailed')), 'error');
        return;
      }
      addToast?.(t('gitEnv.unstashAllDone'), 'success');
      setDiffs({});
      await Promise.all([refresh(), loadStashes()]);
    } finally {
      setBulkBusy(null);
    }
  };

  const confirmDiscardNow = async () => {
    const paths = confirmDiscard;
    setConfirmDiscard(null);
    if (!paths) return;
    setBulkBusy('discard');
    try {
      await runDiscard(paths.length > 0 ? paths : null);
    } finally {
      setBulkBusy(null);
    }
  };

  const discardable = files.filter(canDiscard);
  const frozen = busyPath != null || bulkBusy != null;
  const confirmPaths = confirmDiscard ?? [];
  const confirmWholeRepo = confirmPaths.length === 0;

  return (
    <Overlay scope="inline" open={open} onClose={onClose} backdrop="blur" className={styles.modal} disableContainerAnimation>
      <div className={styles.header}>
        <h2 className={styles.title}>{t('gitEnv.changesTitle')}</h2>
        <button className={styles.closeBtn} onClick={onClose} aria-label="Close">×</button>
      </div>
      <div className={styles.body}>
        {/* 工具条常驻：把改动全部暂存后文件列表会清空，取出/回退的入口不能跟着消失 */}
        <div className={styles.toolbar}>
          <span className={styles.stashCount} data-testid="git-stash-count">
            {t('gitEnv.stashCount', { n: stashes.length })}
          </span>
          <div className={styles.toolbarActions}>
            <button
              type="button"
              className={styles.toolBtn}
              data-testid="git-stash-all"
              disabled={frozen || files.length === 0}
              onClick={() => void handleBulkStash()}
            >
              {bulkBusy === 'stash' ? '…' : t('gitEnv.stashAll')}
            </button>
            <button
              type="button"
              className={styles.toolBtn}
              data-testid="git-unstash-all"
              disabled={frozen || stashes.length === 0}
              onClick={() => void handleBulkUnstash()}
            >
              {bulkBusy === 'unstash' ? '…' : t('gitEnv.unstashAll')}
            </button>
            <button
              type="button"
              className={styles.toolBtnDanger}
              data-testid="git-discard-all"
              disabled={frozen || discardable.length === 0}
              onClick={() => setConfirmDiscard([])}
            >
              {bulkBusy === 'discard' ? '…' : t('gitEnv.discardAll')}
            </button>
          </div>
        </div>

        {files.length === 0 ? (
          <div className={styles.empty} data-testid="git-changes-empty">
            {stashes.length > 0
              ? t('gitEnv.noChangesWithStash', { n: stashes.length })
              : t('gitEnv.noChanges')}
          </div>
        ) : (
          <>
            {files.map(file => {
              const expanded = expandedPath === file.path;
              const diff = diffs[file.path];
              const stashEntry = stashForPath(stashes, file.path);
              const rowBusy = busyPath === file.path;
              return (
                <div key={file.path} className={styles.fileBlock}>
                  <div className={styles.fileRow}>
                    <button
                      type="button"
                      className={styles.fileMain}
                      data-testid={`git-change-${file.path}`}
                      aria-expanded={expanded}
                      onClick={() => void toggleFile(file)}
                    >
                      <Tooltip content={file.path} variant="panel" placement="top" align="start">
                        {({ ref, ...tooltipProps }) => (
                          <span
                            ref={(node) => ref(node)}
                            className={styles.fileName}
                            {...tooltipProps}
                          >
                            {file.path}
                          </span>
                        )}
                      </Tooltip>
                      <span className={styles.fileStats}>
                        <span className={styles.added}>+{fmt(file.additions)}</span>
                        <span className={styles.deleted}>-{fmt(file.deletions)}</span>
                      </span>
                    </button>
                    <div className={styles.fileActions}>
                      <button
                        type="button"
                        className={styles.fileBtn}
                        data-testid={`git-change-stash-${file.path}`}
                        disabled={frozen}
                        onClick={() => void handleStashFile(file)}
                      >
                        {rowBusy ? '…' : t('gitEnv.stashOne')}
                      </button>
                      <Tooltip
                        content={stashEntry ? stashEntry.message : t('gitEnv.unstashNotInStash')}
                        placement="top"
                        disabled={!stashEntry}
                      >
                        {({ ref, ...tooltipProps }) => (
                          <button
                            ref={ref}
                            {...tooltipProps}
                            type="button"
                            className={styles.fileBtn}
                            data-testid={`git-change-unstash-${file.path}`}
                            disabled={frozen || !stashEntry}
                            onClick={() => void handleUnstashFile(file)}
                          >
                            {t('gitEnv.unstashOne')}
                          </button>
                        )}
                      </Tooltip>
                      <Tooltip
                        content={canDiscard(file) ? t('gitEnv.discardOneHint') : t('gitEnv.discardUntrackedHint')}
                        placement="top"
                      >
                        {({ ref, ...tooltipProps }) => (
                          <button
                            ref={ref}
                            {...tooltipProps}
                            type="button"
                            className={styles.fileBtnDanger}
                            data-testid={`git-change-discard-${file.path}`}
                            disabled={frozen || !canDiscard(file)}
                            onClick={() => void handleDiscardFile(file)}
                          >
                            {t('gitEnv.discardOne')}
                          </button>
                        )}
                      </Tooltip>
                    </div>
                  </div>
                  {expanded && (
                    <div className={styles.diffPane} data-testid={`git-diff-${file.path}`}>
                      {loadingPath === file.path && <div className={styles.diffNote}>…</div>}
                      {loadingPath !== file.path && diff === 'error' && (
                        <div className={styles.diffNote}>{t('gitEnv.diffUnavailable')}</div>
                      )}
                      {loadingPath !== file.path && diff && diff !== 'error' && (diff.binary
                        ? <div className={styles.diffNote}>{t('gitEnv.diffBinary')}</div>
                        : <DiffBody patch={diff.patch} truncatedNote={t('gitEnv.diffTruncated')} emptyNote={t('gitEnv.diffUnavailable')} />)}
                    </div>
                  )}
                </div>
              );
            })}
          </>
        )}
      </div>

      <ConfirmDialog
        open={confirmDiscard != null}
        scope="inline"
        title={t('gitEnv.discardConfirmTitle')}
        confirmLabel={t('gitEnv.discardConfirmOk')}
        cancelLabel={t('common.cancel')}
        confirmTone="danger"
        busy={bulkBusy === 'discard'}
        onConfirm={() => void confirmDiscardNow()}
        onCancel={() => setConfirmDiscard(null)}
      >
        <div data-testid="git-discard-confirm-body">
          {confirmWholeRepo
            ? t('gitEnv.discardConfirmAll')
            : t('gitEnv.discardConfirmFiles', { count: confirmPaths.length, list: confirmPaths.join('\n') })}
        </div>
      </ConfirmDialog>
    </Overlay>
  );
}

function DiffBody({ patch, truncatedNote, emptyNote }: { patch: string; truncatedNote: string; emptyNote: string }) {
  if (!patch.trim()) {
    return <div className={styles.diffNote}>{emptyNote}</div>;
  }
  const rows = parseUnifiedPatch(patch);
  const clipped = rows.length > MAX_RENDER_LINES;
  const shown = clipped ? rows.slice(0, MAX_RENDER_LINES) : rows;
  return (
    <>
      <pre className={styles.diff}>
        {shown.map((line, i) => (
          <div
            key={i}
            className={
              line.kind === 'add' ? styles.lineAdded
                : line.kind === 'del' ? styles.lineRemoved
                  : line.kind === 'hunk' ? styles.lineHunk
                    : styles.lineSame
            }
          >
            {line.text || ' '}
          </div>
        ))}
      </pre>
      {clipped && <div className={styles.diffNote}>{truncatedNote}</div>}
    </>
  );
}
