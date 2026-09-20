/**
 * GitEnvironmentCard — 「环境信息」卡（运行信息胶囊内，压平皮肤）
 *
 * 各行（如图）：
 *   Git图谱    未提交变更行合计（+绿/-红，千分位），点击开 Git图谱 弹窗
 *              （提交或推送与变更文件合并后的面板；原「变更」「提交或推送」两行并为一行）
 *   本地       就地展开：该项目全部 worktree（每项两行：分支名 + 路径，带主/当前标记）
 *   新建工作树  开「在 worktree 中开始新会话」弹窗（隔离 worktree + wt/<名称> 分支）
 *   分支       当前分支（截断+箭头），点击弹分支列表，点击分支即切换 / 可直接新建分支
 *
 * 目标目录 = 当前对话工作台的本地根（deskWorkspaceNativeRoot，退 deskBasePath）。
 * 非本地目录不渲染；非 git 目录各行降级禁用。
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { useStore } from '../../stores';
import { applyStudioWorkspace, createLocalStudioWorkspaceFromFolder } from '../../stores/desk-actions';
import { AnchoredPortal, Collapse, Tooltip } from '../../ui';
import {
  fetchGitBranches,
  fetchGitStatus,
  fetchGitWorktreeInfo,
  fetchGitWorktrees,
  type GitBranches,
  type GitStatus,
  type GitWorktreeInfo,
  type GitWorktrees,
} from '../../utils/git-env-api';
import { GitBranchList } from './GitBranchList';
import { GitGraphPanel } from './GitGraphPanel';
import { GitHistoryModal } from './GitHistoryModal';
import { GitWorktreeModal } from './GitWorktreeModal';
import branchStyles from './GitBranchList.module.css';
import styles from './GitEnvironmentCard.module.css';

function fmt(n: number): string {
  return n.toLocaleString('en-US');
}

interface EnvSnapshot {
  status: GitStatus;
  branches: GitBranches;
  worktree: GitWorktreeInfo;
  worktrees: GitWorktrees;
  at: number;
}

/** 按目录缓存最近一次快照。运行信息胶囊关闭时会卸载本卡，重开时若每次都从
 *  空状态重新拉取，用户要盯着「…」等几秒 git 探测；先拿缓存秒出、后台再刷新
 *  （stale-while-revalidate）。仅保留每个目录最新一份。 */
const envSnapshotCache = new Map<string, EnvSnapshot>();

/** 测试隔离用：清空快照缓存（组件模块级状态，vitest 用例间不会自动重置） */
export function resetGitEnvSnapshotCache(): void {
  envSnapshotCache.clear();
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
  const [worktrees, setWorktrees] = useState<GitWorktrees | null>(null);
  const [loadState, setLoadState] = useState<'idle' | 'loading' | 'error'>('idle');
  const [localExpanded, setLocalExpanded] = useState(false);
  const [branchMenuOpen, setBranchMenuOpen] = useState(false);
  const [worktreeModalOpen, setWorktreeModalOpen] = useState(false);
  const [graphOpen, setGraphOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  // 默认折叠（用户裁决）：运行信息容器内只露标题行，点击展开
  const [collapsed, setCollapsed] = useState(true);
  const branchRowRef = useRef<HTMLButtonElement>(null);

  const refresh = useCallback(async (): Promise<GitStatus | null> => {
    if (!dir) return null;
    try {
      const [nextStatus, nextBranches, nextWorktree, nextWorktrees] = await Promise.all([
        fetchGitStatus(dir, currentAgentId),
        fetchGitBranches(dir, currentAgentId),
        fetchGitWorktreeInfo(dir, currentAgentId),
        fetchGitWorktrees(dir, currentAgentId),
      ]);
      setStatus(nextStatus);
      setBranches(nextBranches);
      setWorktree(nextWorktree);
      setWorktrees(nextWorktrees);
      envSnapshotCache.set(dir, { status: nextStatus, branches: nextBranches, worktree: nextWorktree, worktrees: nextWorktrees, at: Date.now() });
      setLoadState('idle');
      return nextStatus;
    } catch {
      setLoadState('error');
      return null;
    }
  }, [dir, currentAgentId]);

  // 工作台切换 → 整体重载；重开胶囊（重挂载）→ 有缓存先秒出再后台刷新；
  // 操作（checkout/commit/push/pull）后由 refresh() 手动刷新
  useEffect(() => {
    const snap = dir ? envSnapshotCache.get(dir) : undefined;
    if (snap) {
      setStatus(snap.status);
      setBranches(snap.branches);
      setWorktree(snap.worktree);
      setWorktrees(snap.worktrees);
      setLoadState('idle');
    } else {
      setStatus(null);
      setBranches(null);
      setWorktree(null);
      setWorktrees(null);
      setLoadState(dir ? 'loading' : 'idle');
    }
    setLocalExpanded(false);
    setBranchMenuOpen(false);
    setWorktreeModalOpen(false);
    if (dir) void refresh();
  }, [dir, refresh]);

  /** 分支浮层里切换/新建成功后：收起浮层并让整卡重新取数 */
  const handleBranchChanged = useCallback(async () => {
    setBranchMenuOpen(false);
    await refresh();
  }, [refresh]);

  /**
   * worktree 建成 → 注册成局部工作台并切过去，落在一个新会话草稿上。
   * 当前检出不受影响：主工作树仍在原分支，新会话跑在 wt/<名称> 上。
   */
  const handleWorktreeCreated = useCallback(async ({ path: worktreePath, branch }: { path: string; branch: string }) => {
    if (!worktreePath) {
      addToast?.(t('gitEnv.worktreeOpenFailed'), 'error');
      return;
    }
    const workspace = await createLocalStudioWorkspaceFromFolder(worktreePath);
    if (!workspace) {
      addToast?.(t('gitEnv.worktreeOpenFailed'), 'error');
      return;
    }
    await applyStudioWorkspace(workspace);
    addToast?.(t('gitEnv.worktreeDone', { name: branch || worktreePath }), 'success');
  }, [addToast, t]);

  if (!dir) return null;

  const isRepo = status?.isRepo ?? false;

  // Git 环境检测不到（探测成功且非 Git 仓库）→ 整卡直接隐藏，不再展示降级行（用户裁决）。
  // status 非空即探测已完成；loading / 探测失败不隐藏——git 仓库不闪跳，失败保留重试行。
  if (status !== null && !status.isRepo) return null;

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
    <section className={`universal-card ${styles.card}`} aria-label={t('gitEnv.title')} data-testid="git-env-card" data-collapsed={collapsed || undefined}>
      <button
        type="button"
        className={styles.headerToggle}
        onClick={() => setCollapsed(v => !v)}
        aria-expanded={!collapsed}
      >
        <span className={styles.title}>{t('gitEnv.title')}</span>
        <Chevron open={!collapsed} className={styles.chevron} />
      </button>
      <Collapse open={!collapsed}>
      <div className={styles.rows}>
        <button
          type="button"
          className={styles.row}
          data-testid="git-env-graph-row"
          disabled={loadState === 'loading' || (loadState === 'idle' && !isRepo)}
          onClick={() => (loadState === 'error' ? void refresh() : setGraphOpen(true))}
        >
          <span className={styles.rowLabel}>{t('gitEnv.graphTitle')}</span>
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
                worktrees?.isRepo && worktrees.worktrees.length > 0 ? (
                  /* 该项目下的全部 worktree，一项两行（分支名 / 路径），不再另起标题 */
                  <div className={styles.worktreeList} data-testid="git-env-worktree-list">
                    {worktrees.worktrees.map(entry => (
                      <div
                        key={entry.path}
                        className={styles.worktreeItem}
                        data-testid={`git-worktree-item-${entry.branch ?? entry.head ?? entry.path}`}
                      >
                        <div className={styles.worktreeHead}>
                          <span className={styles.worktreeName} data-testid="git-worktree-item-name">
                            {entry.branch ?? t('gitEnv.detachedHead', { name: entry.head?.slice(0, 7) ?? '' })}
                          </span>
                          {entry.isMain && <span className={styles.worktreeTag}>{t('gitEnv.worktreeMainTag')}</span>}
                          {entry.current && <span className={styles.worktreeTag}>{t('gitEnv.worktreeCurrentTag')}</span>}
                        </div>
                        <Tooltip content={entry.path} variant="panel" placement="bottom" align="start">
                          {({ ref, ...tooltipProps }) => (
                            <span
                              ref={(node) => ref(node)}
                              className={styles.worktreePath}
                              data-testid="git-worktree-item-path"
                              {...tooltipProps}
                            >
                              {entry.path}
                            </span>
                          )}
                        </Tooltip>
                      </div>
                    ))}
                  </div>
                ) : (
                  /* 降级：worktree 清单取不到时，至少说明当前工作树是哪一棵 */
                  <>
                    <div>
                      {worktree.isMain
                        ? t('gitEnv.mainWorktree')
                        : t('gitEnv.linkedWorktree', { name: worktree.name ?? worktree.branch ?? '' })}
                    </div>
                    {!worktree.isMain && worktree.mainPath && (
                      <Tooltip content={worktree.mainPath} variant="panel" placement="bottom" align="start">
                        {({ ref, ...tooltipProps }) => (
                          <div
                            ref={(node) => ref(node)}
                            className={styles.localPath}
                            {...tooltipProps}
                          >
                            {worktree.mainPath}
                          </div>
                        )}
                      </Tooltip>
                    )}
                  </>
                )
              )}
            </div>
          )}
        </div>

        <button
          type="button"
          className={styles.row}
          data-testid="git-env-worktree-row"
          disabled={!isRepo}
          onClick={() => setWorktreeModalOpen(true)}
        >
          <span className={styles.rowLabel}>{t('gitEnv.worktreeRow')}</span>
          <span className={styles.rowValue}>
            <Chevron open={false} className={styles.chevronFlat} />
          </span>
        </button>

        <button
          type="button"
          ref={branchRowRef}
          className={styles.row}
          data-testid="git-env-branch-row"
          disabled={!isRepo}
          onClick={() => setBranchMenuOpen(v => !v)}
        >
          <span className={styles.rowLabel}>{t('gitEnv.branch')}</span>
          <span className={styles.rowValue}>
            <Tooltip content={branchValue} placement="left" disabled={branchValue === '…'}>
              {({ ref, ...tooltipProps }) => (
                <span
                  ref={(node) => ref(node)}
                  className={styles.branchName}
                  {...tooltipProps}
                >
                  {branchValue}
                </span>
              )}
            </Tooltip>
            <Chevron open={branchMenuOpen} className={styles.chevron} />
          </span>
        </button>

        <button
          type="button"
          className={styles.row}
          data-testid="git-env-history-row"
          disabled={!isRepo}
          onClick={() => setHistoryOpen(true)}
        >
          <span className={styles.rowLabel}>{t('gitEnv.history')}</span>
          <span className={styles.rowValue}>
            <Chevron open={false} className={styles.chevronFlat} />
          </span>
        </button>
      </div>
      </Collapse>

      <AnchoredPortal
        open={branchMenuOpen && isRepo}
        anchorRef={branchRowRef}
        onClose={() => setBranchMenuOpen(false)}
        role="dialog"
        className={`${branchStyles.menu} runtime-capsule-anchored`}
        align="end"
        minWidth={200}
      >
        <GitBranchList
          dir={dir}
          agentId={currentAgentId}
          branches={branches}
          testIdPrefix="git-branch"
          onChanged={handleBranchChanged}
        />
      </AnchoredPortal>

      <GitGraphPanel
        open={graphOpen}
        onClose={() => setGraphOpen(false)}
        dir={dir}
        status={status}
        branches={branches}
        sessionPath={sessionPath}
        agentId={currentAgentId}
        refresh={refresh}
      />
      <GitHistoryModal
        open={historyOpen}
        onClose={() => setHistoryOpen(false)}
        dir={dir}
        agentId={currentAgentId}
      />
      <GitWorktreeModal
        open={worktreeModalOpen}
        onClose={() => setWorktreeModalOpen(false)}
        dir={dir}
        agentId={currentAgentId}
        branches={branches}
        defaultBase={status?.detached ? null : (status?.currentBranch ?? null)}
        worktreesRoot={worktrees?.root ?? null}
        onCreated={handleWorktreeCreated}
      />
    </section>
  );
}
