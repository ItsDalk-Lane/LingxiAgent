/**
 * GitBranchPicker — 输入框上方的分支下拉
 *
 * 分支切换的入口（原运行信息弹窗·环境信息卡·分支行迁移至此）：
 *   - 控件在输入框上方一行内（IDE 风）：分支图标 + 当前分支名 + ▾，无边框胶囊
 *     （分离头指针时显示缩写说明）；非 Git 仓库 / 探测中 / 失败不渲染
 *   - 点击展开分支 × 工作树合并菜单（GitBranchList）：检出在某工作树的分支带「主/独」
 *     标志（悬停看路径）、未连接分支的工作树单列末尾
 *   - 底部按钮：＋新建工作树（开 GitWorktreeModal，建成注册工作台并开新会话）
 *     → ＋新建分支 → 提交记录（原环境信息卡·提交记录行迁入，开 GitHistoryModal）
 * 数据走 useGitEnv 共享层；挂在 InputArea 的 input-stack 内（输入卡片上方）。
 */
import { useCallback, useRef, useState } from 'react';
import { useStore } from '../../stores';
import { applyStudioWorkspace, createLocalStudioWorkspaceFromFolder } from '../../stores/desk-actions';
import { AnchoredPortal } from '../../ui';
import { useGitEnv } from '../../hooks/use-git-env';
import { GitBranchList } from './GitBranchList';
import { GitHistoryModal } from './GitHistoryModal';
import { GitWorktreeModal } from './GitWorktreeModal';
import branchStyles from './GitBranchList.module.css';
import styles from './GitBranchPicker.module.css';

export function GitBranchPicker() {
  const t = window.t ?? ((p: string) => p);
  const dir = useStore(s => s.deskWorkspaceNativeRoot || s.deskBasePath);
  const agentId = useStore(s => s.currentAgentId);
  const addToast = useStore(s => s.addToast);
  const { status, branches, worktrees, refresh } = useGitEnv(dir, agentId);
  const [menuOpen, setMenuOpen] = useState(false);
  const [worktreeModalOpen, setWorktreeModalOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const pillRef = useRef<HTMLButtonElement>(null);

  /** 菜单里切换/新建成功后：收浮层并刷新共享数据 */
  const handleChanged = useCallback(async () => {
    setMenuOpen(false);
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

  // 探测完成且是 Git 仓库才渲染：loading / 非 git / 失败都不占输入框左上角
  if (!status?.isRepo) return null;

  const branchValue = status.detached
    ? t('gitEnv.detachedHead', { name: status.currentBranch ?? '' })
    : (status.currentBranch ?? '—');

  return (
    <>
      <button
        type="button"
        ref={pillRef}
        className={styles.picker}
        data-testid="branch-picker-pill"
        aria-haspopup="menu"
        aria-label={`${t('gitEnv.branch')} ${branchValue}`}
        aria-expanded={menuOpen}
        title={branchValue}
        onClick={() => setMenuOpen(v => !v)}
      >
        <svg className={styles.pickerIcon} width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <line x1="6" y1="3" x2="6" y2="15" />
          <circle cx="18" cy="6" r="3" />
          <circle cx="6" cy="18" r="3" />
          <path d="M18 9a9 9 0 0 1-9 9" />
        </svg>
        <span className={styles.pickerName}>{branchValue}</span>
        <svg className={styles.pickerChevron} data-open={menuOpen} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>

      <AnchoredPortal
        open={menuOpen}
        anchorRef={pillRef}
        onClose={() => setMenuOpen(false)}
        role="dialog"
        className={`${branchStyles.menu} runtime-capsule-anchored`}
        align="start"
        minWidth={200}
      >
        <GitBranchList
          dir={dir!}
          agentId={agentId}
          branches={branches}
          worktrees={worktrees}
          testIdPrefix="git-branch"
          createWorktreeSlot={
            <button
              type="button"
              className={styles.menuAction}
              data-testid="git-env-worktree-create"
              onClick={() => {
                // 先收浮层再开弹窗：浮层挂在胶囊锚点上，留在原地会挡住弹窗
                setMenuOpen(false);
                setWorktreeModalOpen(true);
              }}
            >
              <span className={styles.menuActionGlyph} aria-hidden="true">＋</span>
              <span>{t('gitEnv.worktreeRow')}</span>
            </button>
          }
          historySlot={
            /* 提交记录入口（原环境信息卡·提交记录行迁入菜单底部） */
            <button
              type="button"
              className={styles.menuAction}
              data-testid="git-branch-history"
              onClick={() => {
                setMenuOpen(false);
                setHistoryOpen(true);
              }}
            >
              <span className={styles.menuActionGlyph} aria-hidden="true">≡</span>
              <span>{t('gitEnv.history')}</span>
            </button>
          }
          onChanged={handleChanged}
        />
      </AnchoredPortal>

      <GitWorktreeModal
        open={worktreeModalOpen}
        onClose={() => setWorktreeModalOpen(false)}
        dir={dir!}
        agentId={agentId}
        branches={branches}
        defaultBase={status.detached ? null : (status.currentBranch ?? null)}
        worktreesRoot={worktrees?.root ?? null}
        onCreated={handleWorktreeCreated}
      />
      <GitHistoryModal
        open={historyOpen}
        onClose={() => setHistoryOpen(false)}
        dir={dir!}
        agentId={agentId}
      />
    </>
  );
}
