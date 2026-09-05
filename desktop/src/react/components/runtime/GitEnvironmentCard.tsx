/**
 * GitEnvironmentCard — 「环境信息」卡（运行信息胶囊内，压平皮肤）
 *
 * 四行（如图）：
 *   变更       未提交变更行合计（+绿/-红，千分位），点击开变更文件弹窗
 *   本地       就地展开：本地主工作树 / 分支工作树
 *   分支       当前分支（截断+箭头），点击弹分支列表，点击分支即切换
 *   提交或推送  点击开提交弹窗（提交 / 提交并推送 / 推送）
 *
 * 目标目录 = 当前对话工作台的本地根（deskWorkspaceNativeRoot，退 deskBasePath）。
 * 非本地目录不渲染；非 git 目录四行降级禁用。
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { useStore } from '../../stores';
import { AnchoredPortal } from '../../ui';
import {
  fetchGitBranches,
  fetchGitStatus,
  fetchGitWorktreeInfo,
  gitCheckout,
  type GitBranches,
  type GitStatus,
  type GitWorktreeInfo,
} from '../../utils/git-env-api';
import { GitChangesModal } from './GitChangesModal';
import { GitCommitModal } from './GitCommitModal';
import styles from './GitEnvironmentCard.module.css';

function fmt(n: number): string {
  return n.toLocaleString('en-US');
}

function Chevron({ open, className }: { open: boolean; className?: string }) {
  return (
    <svg className={className} data-open={open} width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="6 9 12 15 18 9" />
    </svg>
  );
}

export function GitEnvironmentCard() {
  const t = window.t ?? ((p: string) => p);
  const dir = useStore(s => s.deskWorkspaceNativeRoot || s.deskBasePath);
  const sessionPath = useStore(s => s.currentSessionPath);
  const currentAgentId = useStore(s => s.currentAgentId);
  const addToast = useStore(s => s.addToast);

  const [status, setStatus] = useState<GitStatus | null>(null);
  const [branches, setBranches] = useState<GitBranches | null>(null);
  const [worktree, setWorktree] = useState<GitWorktreeInfo | null>(null);
  const [loadState, setLoadState] = useState<'idle' | 'loading' | 'error'>('idle');
  const [localExpanded, setLocalExpanded] = useState(false);
  const [branchMenuOpen, setBranchMenuOpen] = useState(false);
  const [switchingBranch, setSwitchingBranch] = useState<string | null>(null);
  const [changesOpen, setChangesOpen] = useState(false);
  const [commitOpen, setCommitOpen] = useState(false);
  const branchRowRef = useRef<HTMLButtonElement>(null);

  const refresh = useCallback(async (): Promise<GitStatus | null> => {
    if (!dir) return null;
    try {
      const [nextStatus, nextBranches, nextWorktree] = await Promise.all([
        fetchGitStatus(dir, currentAgentId),
        fetchGitBranches(dir, currentAgentId),
        fetchGitWorktreeInfo(dir, currentAgentId),
      ]);
      setStatus(nextStatus);
      setBranches(nextBranches);
      setWorktree(nextWorktree);
      setLoadState('idle');
      return nextStatus;
    } catch {
      setLoadState('error');
      return null;
    }
  }, [dir, currentAgentId]);

  // 工作台切换 → 整体重载；操作（checkout/commit/push）后由 refresh() 手动刷新
  useEffect(() => {
    setStatus(null);
    setBranches(null);
    setWorktree(null);
    setLocalExpanded(false);
    setBranchMenuOpen(false);
    setLoadState(dir ? 'loading' : 'idle');
    if (dir) void refresh();
  }, [dir, refresh]);

  const handleSwitchBranch = useCallback(async (name: string) => {
    if (!dir || switchingBranch) return;
    setSwitchingBranch(name);
    try {
      const result = await gitCheckout(dir, name, currentAgentId);
      if (result.httpOk && result.ok) {
        addToast?.(t('gitEnv.switchDone', { name }), 'success');
        setBranchMenuOpen(false);
        await refresh();
      } else {
        addToast?.(result.error || t('gitEnv.switchFailed'), 'error');
      }
    } finally {
      setSwitchingBranch(null);
    }
  }, [dir, switchingBranch, refresh, addToast, t, currentAgentId]);

  if (!dir) return null;

  const isRepo = status?.isRepo ?? false;

  const changesValue = (() => {
    if (loadState === 'loading') return '…';
    if (loadState === 'error') return t('gitEnv.loadFailed');
    if (!status || !status.isRepo) return t('gitEnv.notGitRepo');
    return null; // 走增删渲染
  })();

  const localValue = worktree?.isRepo
    ? (worktree.isMain ? t('gitEnv.mainWorktree') : t('gitEnv.linkedWorktreeShort'))
    : (loadState === 'loading' ? '…' : '—');

  const branchValue = status?.isRepo
    ? (status.detached
        ? t('gitEnv.detachedHead', { name: status.currentBranch ?? '' })
        : status.currentBranch ?? '—')
    : (loadState === 'loading' ? '…' : '—');

  return (
    <section className={`universal-card ${styles.card}`} aria-label={t('gitEnv.title')} data-testid="git-env-card">
      <div className={styles.header}>
        <span className={styles.title}>{t('gitEnv.title')}</span>
      </div>
      <div className={styles.rows}>
        <button
          type="button"
          className={styles.row}
          data-testid="git-env-changes-row"
          disabled={loadState === 'loading' || (loadState === 'idle' && !isRepo)}
          title={loadState === 'error' ? t('gitEnv.loadFailed') : undefined}
          onClick={() => (loadState === 'error' ? void refresh() : setChangesOpen(true))}
        >
          <span className={styles.rowLabel}>{t('gitEnv.changes')}</span>
          <span className={styles.rowValue}>
            {changesValue ?? (
              <>
                <span className={styles.added}>+{fmt(status!.total.additions)}</span>
                <span className={styles.deleted}>-{fmt(status!.total.deletions)}</span>
              </>
            )}
          </span>
        </button>

        <div className={styles.localBlock}>
          <button
            type="button"
            className={styles.row}
            data-testid="git-env-local-row"
            disabled={!isRepo}
            aria-expanded={localExpanded}
            onClick={() => setLocalExpanded(v => !v)}
          >
            <span className={styles.rowLabel}>{t('gitEnv.local')}</span>
            <span className={styles.rowValue}>
              {localValue}
              <Chevron open={localExpanded} className={styles.chevron} />
            </span>
          </button>
          {localExpanded && (
            <div className={styles.localDetail} data-testid="git-env-local-detail">
              {worktree?.isRepo && (
                <>
                  <div>
                    {worktree.isMain
                      ? t('gitEnv.mainWorktree')
                      : t('gitEnv.linkedWorktree', { name: worktree.name ?? worktree.branch ?? '' })}
                  </div>
                  {!worktree.isMain && worktree.mainPath && (
                    <div className={styles.localPath} title={worktree.mainPath}>{worktree.mainPath}</div>
                  )}
                </>
              )}
            </div>
          )}
        </div>

        <button
          type="button"
          ref={branchRowRef}
          className={styles.row}
          data-testid="git-env-branch-row"
          disabled={!isRepo}
          title={branchValue === '…' ? undefined : branchValue}
          onClick={() => setBranchMenuOpen(v => !v)}
        >
          <span className={styles.rowLabel}>{t('gitEnv.branch')}</span>
          <span className={styles.rowValue}>
            <span className={styles.branchName}>{branchValue}</span>
            <Chevron open={branchMenuOpen} className={styles.chevron} />
          </span>
        </button>

        <button
          type="button"
          className={styles.row}
          data-testid="git-env-commit-row"
          disabled={!isRepo}
          onClick={() => setCommitOpen(true)}
        >
          <span className={styles.rowLabel}>{t('gitEnv.commitOrPush')}</span>
          <span className={styles.rowValue}>
            <Chevron open={false} className={styles.chevronFlat} />
          </span>
        </button>
      </div>

      <AnchoredPortal
        open={branchMenuOpen && isRepo}
        anchorRef={branchRowRef}
        onClose={() => setBranchMenuOpen(false)}
        role="dialog"
        className={`${styles.branchMenu} runtime-capsule-anchored`}
        align="end"
        minWidth={200}
      >
        <div className={styles.branchMenuTitle}>{t('gitEnv.branchesTitle')}</div>
        <div className={styles.branchList}>
          {(branches?.branches ?? []).map(branch => (
            <button
              key={branch.name}
              type="button"
              className={`${styles.branchItem}${branch.current ? ` ${styles.branchItemCurrent}` : ''}`}
              data-testid={`git-branch-${branch.name}`}
              disabled={branch.current || branch.checkedOutElsewhere || switchingBranch != null}
              title={branch.checkedOutElsewhere ? t('gitEnv.checkedOutElsewhere') : undefined}
              onClick={() => void handleSwitchBranch(branch.name)}
            >
              <span className={styles.branchItemName}>{branch.name}</span>
              {switchingBranch === branch.name && <span className={styles.branchBusy}>…</span>}
              {branch.current && <span className={styles.branchCurrentMark}>✓</span>}
            </button>
          ))}
          {branches != null && branches.branches.length === 0 && (
            <div className={styles.branchEmpty}>{t('gitEnv.noBranches')}</div>
          )}
        </div>
      </AnchoredPortal>

      <GitChangesModal
        open={changesOpen}
        onClose={() => setChangesOpen(false)}
        dir={dir}
        files={status?.files ?? []}
      />
      <GitCommitModal
        open={commitOpen}
        onClose={() => setCommitOpen(false)}
        dir={dir}
        status={status}
        branches={branches}
        sessionPath={sessionPath}
        agentId={currentAgentId}
        refresh={refresh}
      />
    </section>
  );
}
