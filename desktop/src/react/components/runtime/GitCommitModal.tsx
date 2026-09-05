/**
 * GitCommitModal — 提交或推送弹窗（环境信息卡·提交或推送行入口）
 *
 * 顶部分支条（下拉可切换分支）；提交信息输入（留空 → AI 生成并回填）；
 * 「包含未暂存的更改」勾选（默认勾选，右侧显示未暂存增删统计）；
 * 底部三操作：提交 / 提交并推送 / 推送。
 *   - 提交、提交并推送：无可提交内容时置灰
 *   - 推送：无可推送提交（且未配置建立跟踪的远程）时置灰
 *   - 提交并推送 = 先提交（若无可提交则跳过）再推送
 */
import { useEffect, useRef, useState } from 'react';
import { AnchoredPortal, Overlay, Tooltip } from '../../ui';
import { useStore } from '../../stores';
import {
  generateGitCommitMessage,
  gitCheckout,
  gitCommit,
  gitPush,
  type GitActionResult,
  type GitBranches,
  type GitStatus,
} from '../../utils/git-env-api';
import styles from './GitCommitModal.module.css';

type BusyStep = null | 'ai' | 'commit' | 'commit-push' | 'push' | 'checkout';

function fmt(n: number): string {
  return n.toLocaleString('en-US');
}

interface GitCommitModalProps {
  open: boolean;
  onClose: () => void;
  dir: string;
  status: GitStatus | null;
  branches: GitBranches | null;
  sessionPath: string | null;
  agentId: string | null;
  refresh: () => Promise<GitStatus | null>;
}

export function GitCommitModal({
  open, onClose, dir, status, branches, sessionPath, agentId, refresh,
}: GitCommitModalProps) {
  const t = window.t ?? ((p: string) => p);
  const addToast = useStore(s => s.addToast);
  const [message, setMessage] = useState('');
  const [includeUnstaged, setIncludeUnstaged] = useState(true);
  const [busy, setBusy] = useState<BusyStep>(null);
  const [branchMenuOpen, setBranchMenuOpen] = useState(false);
  const branchButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) {
      setMessage('');
      setIncludeUnstaged(true);
      setBusy(null);
      setBranchMenuOpen(false);
    }
  }, [open]);

  const commitable = Boolean(status?.isRepo && status.commitable);
  const pushable = Boolean(status?.isRepo && status.pushable);
  const unstaged = status?.unstagedTotal ?? { additions: 0, deletions: 0 };
  const branchLabel = status?.isRepo
    ? (status.detached
        ? t('gitEnv.detachedHead', { name: status.currentBranch ?? '' })
        : status.currentBranch ?? '—')
    : '—';

  const describeResult = (result: GitActionResult): string => {
    if (result.code === 'nothing_staged') return t('gitEnv.nothingStaged');
    if (result.code === 'nothing_to_commit') return t('gitEnv.nothingToCommit');
    if (result.code === 'nothing_to_push') return t('gitEnv.nothingToPush');
    if (result.code === 'no_remote') return t('gitEnv.noRemote');
    const detail = result.message || result.error;
    return detail ? `${t('gitEnv.operationFailed')}: ${detail}` : t('gitEnv.operationFailed');
  };

  /** 提交信息来源：输入框优先，留空走 AI 生成并回填 */
  const obtainMessage = async (): Promise<string | null> => {
    const trimmed = message.trim();
    if (trimmed) return trimmed;
    setBusy('ai');
    const ai = await generateGitCommitMessage(dir, { includeUnstaged, sessionPath, agentId });
    if (!ai.httpOk || !ai.message) {
      addToast?.(ai.error || t('gitEnv.aiFailed'), 'error');
      return null;
    }
    setMessage(ai.message);
    return ai.message;
  };

  const runCommit = async (): Promise<boolean> => {
    const msg = await obtainMessage();
    if (msg == null) return false;
    setBusy('commit');
    const result = await gitCommit(dir, { message: msg, includeUnstaged, agentId });
    if (!result.httpOk || !result.ok) {
      addToast?.(describeResult(result), 'error');
      return false;
    }
    return true;
  };

  const runPush = async (silentNothingToPush: boolean): Promise<boolean> => {
    const result = await gitPush(dir, agentId);
    if (result.httpOk && result.ok) return true;
    if (silentNothingToPush && result.code === 'nothing_to_push') return true;
    addToast?.(describeResult(result), 'error');
    return false;
  };

  const handleCommit = async () => {
    if (busy || !commitable) return;
    setBusy('commit');
    try {
      if (!(await runCommit())) return;
      addToast?.(t('gitEnv.commitDone'), 'success');
      await refresh();
      onClose();
    } finally {
      setBusy(null);
    }
  };

  const handleCommitAndPush = async () => {
    if (busy || (!commitable && !pushable)) return;
    setBusy('commit-push');
    try {
      if (commitable) {
        if (!(await runCommit())) return;
        addToast?.(t('gitEnv.commitDone'), 'success');
        // 提交改变了 ahead，推送资格以刷新后的状态为准
        const fresh = await refresh();
        if (fresh && !fresh.pushable) return;
      }
      setBusy('push');
      if (!(await runPush(true))) return;
      addToast?.(t('gitEnv.pushDone'), 'success');
      await refresh();
      onClose();
    } finally {
      setBusy(null);
    }
  };

  const handlePush = async () => {
    if (busy || !pushable) return;
    setBusy('push');
    try {
      if (!(await runPush(false))) return;
      addToast?.(t('gitEnv.pushDone'), 'success');
      await refresh();
      onClose();
    } finally {
      setBusy(null);
    }
  };

  const handleSwitchBranch = async (name: string) => {
    if (busy) return;
    setBusy('checkout');
    try {
      const result = await gitCheckout(dir, name, agentId);
      if (result.httpOk && result.ok) {
        setBranchMenuOpen(false);
        await refresh();
      } else {
        addToast?.(result.error || t('gitEnv.switchFailed'), 'error');
      }
    } finally {
      setBusy(null);
    }
  };

  const commitLabel = busy === 'ai' ? t('gitEnv.aiGenerating') : t('gitEnv.btnCommit');
  const commitPushLabel = busy === 'ai' ? t('gitEnv.aiGenerating') : t('gitEnv.btnCommitPush');

  return (
    <Overlay scope="inline" open={open} onClose={onClose} backdrop="blur" className={styles.modal} disableContainerAnimation>
      <div className={styles.header}>
        <h2 className={styles.title}>{t('gitEnv.commitTitle')}</h2>
        <button className={styles.closeBtn} onClick={onClose} aria-label="Close">×</button>
      </div>
      <div className={styles.body}>
        <button
          type="button"
          ref={branchButtonRef}
          className={styles.branchButton}
          data-testid="git-commit-branch"
          onClick={() => setBranchMenuOpen(v => !v)}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true">
            <circle cx="6" cy="6" r="2.6" />
            <circle cx="6" cy="18" r="2.6" />
            <circle cx="18" cy="8" r="2.6" />
            <path d="M6 8.6v6.8M17 10.5c0 3.4-4.5 3.9-8.6 4.3" />
          </svg>
          <Tooltip content={branchLabel} placement="top">
            {({ ref, ...tooltipProps }) => (
              <span
                ref={(node) => ref(node)}
                className={styles.branchName}
                {...tooltipProps}
              >
                {branchLabel}
              </span>
            )}
          </Tooltip>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </button>

        <label className={styles.checkRow}>
          <input
            type="checkbox"
            checked={includeUnstaged}
            onChange={e => setIncludeUnstaged(e.target.checked)}
          />
          <span className={styles.checkLabel}>{t('gitEnv.includeUnstaged')}</span>
          <span className={styles.unstagedStats} data-testid="git-commit-unstaged-stats">
            <span className={styles.added}>+{fmt(unstaged.additions)}</span>
            <span className={styles.deleted}>-{fmt(unstaged.deletions)}</span>
          </span>
        </label>

        <textarea
          className={styles.messageInput}
          data-testid="git-commit-message"
          rows={4}
          value={message}
          onChange={e => setMessage(e.target.value)}
          placeholder={t('gitEnv.commitMessagePlaceholder')}
          disabled={busy != null}
        />

        <div className={styles.actions}>
          <button
            type="button"
            className={styles.secondaryBtn}
            data-testid="git-commit-btn"
            disabled={!commitable || busy != null}
            onClick={() => void handleCommit()}
          >
            {commitLabel}
          </button>
          <button
            type="button"
            className={styles.secondaryBtn}
            data-testid="git-commit-push-btn"
            disabled={(!commitable && !pushable) || busy != null}
            onClick={() => void handleCommitAndPush()}
          >
            {commitPushLabel}
          </button>
          <button
            type="button"
            className={styles.primaryBtn}
            data-testid="git-push-btn"
            disabled={!pushable || busy != null}
            onClick={() => void handlePush()}
          >
            {busy === 'push' || busy === 'commit-push' ? `${t('gitEnv.btnPush')}…` : t('gitEnv.btnPush')}
          </button>
        </div>
      </div>

      <AnchoredPortal
        open={branchMenuOpen}
        anchorRef={branchButtonRef}
        onClose={() => setBranchMenuOpen(false)}
        role="dialog"
        className={`${styles.branchMenu} runtime-capsule-anchored`}
        align="start"
        minWidth={220}
      >
        <div className={styles.branchMenuTitle}>{t('gitEnv.branchesTitle')}</div>
        <div className={styles.branchList}>
          {(branches?.branches ?? []).map(branch => (
            <button
              key={branch.name}
              type="button"
              className={`${styles.branchItem}${branch.current ? ` ${styles.branchItemCurrent}` : ''}`}
              data-testid={`git-commit-branch-${branch.name}`}
              disabled={branch.current || branch.checkedOutElsewhere || busy != null}
              title={branch.checkedOutElsewhere ? t('gitEnv.checkedOutElsewhere') : undefined}
              onClick={() => void handleSwitchBranch(branch.name)}
            >
              <span className={styles.branchItemName}>{branch.name}</span>
              {branch.current && <span className={styles.branchCurrentMark}>✓</span>}
            </button>
          ))}
        </div>
      </AnchoredPortal>
    </Overlay>
  );
}
