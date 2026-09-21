/**
 * GitBranchList — 分支浮层内容（输入框分支下拉 / 提交或推送弹窗·分支条共用）
 *
 * 只负责浮层「内容」与交互：切换分支、在当前 HEAD 上新建并检出分支。
 * 浮层容器（AnchoredPortal）仍由调用方持有，锚点对齐与关闭策略各归各的。
 *
 * 首行不设标题：分支项的上一行不再重复「切换分支」抬头，浮层第一行就是分支本身。
 * 分支项整行可点（宽度撑满），悬停/键盘焦点有行级高亮，便于确认当前指着的分支。
 */
import { useMemo, useState, type ReactNode } from 'react';
import { useStore } from '../../stores';
import { Tooltip } from '../../ui';
import { gitCheckout, gitCreateBranch, type GitBranches, type GitWorktreeEntry, type GitWorktrees } from '../../utils/git-env-api';
import styles from './GitBranchList.module.css';

interface GitBranchListProps {
  dir: string;
  agentId: string | null;
  branches: GitBranches | null;
  /** 外部忙碌（如提交弹窗正在提交）：整个列表冻结 */
  busy?: boolean;
  /** 分支项 testid 前缀：`<prefix>-<branch>`；两个调用方各自沿用既有 id */
  testIdPrefix: string;
  /** 提供时启用分支 × 工作树合并显示：检出在某工作树的分支名后带标志（悬停看路径），
   *  未连接分支的工作树单列在列表末尾 */
  worktrees?: GitWorktrees | null;
  /** 底部按钮区顶部的额外入口（如「新建工作树」），在「新建分支」上方同行排布 */
  createWorktreeSlot?: ReactNode;
  /** 底部按钮区末尾的额外入口（如「提交记录」），在「新建分支」下方 */
  historySlot?: ReactNode;
  /** 切换 / 新建成功后回调：调用方负责关闭浮层并刷新数据 */
  onChanged: () => void | Promise<void>;
}

export function GitBranchList({
  dir,
  agentId,
  branches,
  busy = false,
  testIdPrefix,
  worktrees = null,
  createWorktreeSlot,
  historySlot,
  onChanged,
}: GitBranchListProps) {
  const t = window.t ?? ((p: string) => p);
  const addToast = useStore(s => s.addToast);
  const [switching, setSwitching] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState('');
  const [creating, setCreating] = useState(false);

  const frozen = busy || switching != null || creating;

  // 分支 × 工作树合并：按分支名找到检出该分支的工作树；没挂分支（detached）的单独列在末尾
  const { worktreeByBranch, detachedWorktrees } = useMemo(() => {
    const byBranch = new Map<string, GitWorktreeEntry>();
    const detached: GitWorktreeEntry[] = [];
    if (worktrees?.isRepo) {
      for (const entry of worktrees.worktrees) {
        if (entry.branch) byBranch.set(entry.branch, entry);
        else if (entry.detached) detached.push(entry);
      }
    }
    return { worktreeByBranch: byBranch, detachedWorktrees: detached };
  }, [worktrees]);

  const handleSwitch = async (branch: string) => {
    if (frozen) return;
    setSwitching(branch);
    try {
      const result = await gitCheckout(dir, branch, agentId);
      if (result.httpOk && result.ok) {
        addToast?.(t('gitEnv.switchDone', { name: branch }), 'success');
        await onChanged();
      } else {
        addToast?.(result.error || t('gitEnv.switchFailed'), 'error');
      }
    } finally {
      setSwitching(null);
    }
  };

  /** 新建分支的失败码 → 本地化文案（git 的 stderr 优先，便于用户看到真实原因） */
  const describeCreateFailure = (code?: string, message?: string): string => {
    if (code === 'invalid_name') return t('gitEnv.invalidBranchName');
    if (code === 'already_exists') return t('gitEnv.branchExists');
    if (code === 'invalid_base') return t('gitEnv.branchMissing');
    return message || t('gitEnv.createBranchFailed');
  };

  const handleCreate = async () => {
    const trimmed = name.trim();
    if (!trimmed || frozen) return;
    setCreating(true);
    try {
      const result = await gitCreateBranch(dir, trimmed, undefined, agentId);
      if (result.httpOk && result.ok) {
        addToast?.(t('gitEnv.createBranchDone', { name: result.branch || trimmed }), 'success');
        setName('');
        setCreateOpen(false);
        await onChanged();
      } else {
        addToast?.(describeCreateFailure(result.code, result.message), 'error');
      }
    } finally {
      setCreating(false);
    }
  };

  const closeCreate = () => {
    setCreateOpen(false);
    setName('');
  };

  return (
    <>
      <div className={styles.list} data-testid={`${testIdPrefix}-list`}>
        {(branches?.branches ?? []).map(branch => {
          const linkedWorktree = worktreeByBranch.get(branch.name);
          return (
            <Tooltip
              key={branch.name}
              content={branch.checkedOutElsewhere ? t('gitEnv.checkedOutElsewhere') : ''}
              placement="left"
              disabled={!branch.checkedOutElsewhere}
            >
              {({ ref, ...tooltipProps }) => (
                <button
                  ref={ref}
                  {...tooltipProps}
                  type="button"
                  className={`${styles.item}${branch.current ? ` ${styles.itemCurrent}` : ''}`}
                  data-testid={`${testIdPrefix}-${branch.name}`}
                  disabled={branch.current || branch.checkedOutElsewhere || frozen}
                  aria-current={branch.current || undefined}
                  onClick={() => void handleSwitch(branch.name)}
                >
                  <span className={styles.itemName}>{branch.name}</span>
                  {switching === branch.name && <span className={styles.itemBusy}>…</span>}
                  {linkedWorktree && (
                    /* 工作树标志：「主」＝检出在主工作树（与主分支共用同一棵），
                       「独」＝该分支自己的独立工作树；路径不直接展示，悬停标志才看得到 */
                    <Tooltip content={linkedWorktree.path} placement="left">
                      {({ ref: badgeRef, ...badgeProps }) => (
                        <span
                          ref={badgeRef}
                          {...badgeProps}
                          className={styles.worktreeBadge}
                          data-testid={`${testIdPrefix}-worktree-badge`}
                        >
                          {linkedWorktree.isMain ? t('gitEnv.worktreeMainTag') : t('gitEnv.worktreeLinkedTag')}
                        </span>
                      )}
                    </Tooltip>
                  )}
                  {branch.current && <span className={styles.itemMark}>✓</span>}
                </button>
              )}
            </Tooltip>
          );
        })}
        {branches != null && branches.branches.length === 0 && (
          <div className={styles.empty}>{t('gitEnv.noBranches')}</div>
        )}
        {detachedWorktrees.map(entry => (
          /* 没挂在任何分支上的工作树：单列一项，悬停看路径 */
          <Tooltip key={entry.path} content={entry.path} placement="left">
            {({ ref, ...tooltipProps }) => (
              <div
                ref={ref}
                {...tooltipProps}
                className={`${styles.item} ${styles.detachedWorktree}`}
                data-testid={`${testIdPrefix}-worktree-no-branch`}
              >
                <span className={styles.itemName}>{t('gitEnv.worktreeNoBranch')}</span>
              </div>
            )}
          </Tooltip>
        ))}
      </div>

      <div className={styles.createRow} data-testid={`${testIdPrefix}-create-row`}>
        {createWorktreeSlot}
        {createOpen ? (
          <form
            className={styles.createForm}
            onSubmit={(e) => { e.preventDefault(); void handleCreate(); }}
          >
            <input
              className={styles.createInput}
              data-testid={`${testIdPrefix}-create-input`}
              value={name}
              autoFocus
              spellCheck={false}
              placeholder={t('gitEnv.newBranchPlaceholder')}
              disabled={creating}
              onChange={e => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key !== 'Escape') return;
                // 先收起输入行本身：Escape 不该顺带把整个浮层关掉
                e.stopPropagation();
                e.preventDefault();
                closeCreate();
              }}
            />
            <button
              type="submit"
              className={styles.createSubmit}
              data-testid={`${testIdPrefix}-create-submit`}
              disabled={!name.trim() || frozen}
            >
              {creating ? '…' : t('gitEnv.createBranch')}
            </button>
          </form>
        ) : (
          <button
            type="button"
            className={styles.createToggle}
            data-testid={`${testIdPrefix}-create-toggle`}
            disabled={frozen}
            onClick={() => setCreateOpen(true)}
          >
            <span className={styles.plus} aria-hidden="true">＋</span>
            <span>{t('gitEnv.newBranch')}</span>
          </button>
        )}
        {historySlot}
      </div>
    </>
  );
}
