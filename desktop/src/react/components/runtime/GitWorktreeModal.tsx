/**
 * GitWorktreeModal — 「在 worktree 中开始新会话」创建界面
 *
 * 在 <主工作树父级>/worktrees/<名称> 建一个隔离工作树，携带新分支 wt/<名称>，
 * 基线分支可选（默认当前分支）。建成后由调用方把该目录注册成工作台并切过去开
 * 新会话——当前检出不受影响。
 *
 * 只是创建入口：本组件不碰会话，只负责取名、选基线、调 /api/git/worktree-create。
 */
import { useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../../stores';
import { Overlay } from '../../ui';
import { gitCreateWorktree, type GitActionResult, type GitBranches } from '../../utils/git-env-api';
import styles from './GitWorktreeModal.module.css';

/** 与 server/git/git-command.ts 的 isValidWorktreeName 同规则：先在本地拦一道明显非法名 */
const WORKTREE_NAME_RE = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
/** 描述句里的路径占位符。先把它替换成一个哨兵再切开，路径片段才能内嵌高亮；
 *  直接留 `{root}` 会被 t() 的插值规则先吃掉，位置就找不回来了。 */
const ROOT_SLOT = '\u0000worktrees-root\u0000';

interface GitWorktreeModalProps {
  open: boolean;
  onClose: () => void;
  dir: string;
  agentId: string | null;
  branches: GitBranches | null;
  /** 基线分支缺省值（通常是当前分支） */
  defaultBase: string | null;
  /** worktree 落地根目录（<主工作树父级>/worktrees），未知时不展示路径片段 */
  worktreesRoot: string | null;
  /** 创建成功：调用方负责把该目录作为工作台打开新会话 */
  onCreated: (created: { path: string; branch: string }) => void | Promise<void>;
}

export function GitWorktreeModal({
  open, onClose, dir, agentId, branches, defaultBase, worktreesRoot, onCreated,
}: GitWorktreeModalProps) {
  const t = window.t ?? ((p: string) => p);
  const addToast = useStore(s => s.addToast);
  const [name, setName] = useState('');
  const [baseOverride, setBaseOverride] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  const branchNames = useMemo(
    () => (branches?.branches ?? []).map(branch => branch.name),
    [branches],
  );
  const baseOptions = branchNames.length > 0 ? branchNames : (defaultBase ? [defaultBase] : []);
  const fallbackBase = defaultBase && branchNames.includes(defaultBase)
    ? defaultBase
    : (branchNames[0] ?? defaultBase ?? '');
  const base = baseOverride ?? fallbackBase;

  useEffect(() => {
    if (open) return;
    setName('');
    setBaseOverride(null);
    setBusy(false);
  }, [open]);

  const trimmed = name.trim();
  const nameValid = WORKTREE_NAME_RE.test(trimmed);
  const showNameHint = trimmed.length > 0 && !nameValid;
  const canSubmit = nameValid && !busy;

  const describeFailure = (result: GitActionResult): string => {
    if (result.code === 'invalid_name') return t('gitEnv.invalidWorktreeName');
    if (result.code === 'exists') return t('gitEnv.worktreeDirExists');
    if (result.code === 'branch_exists') return t('gitEnv.worktreeBranchExists');
    if (result.code === 'invalid_base') return t('gitEnv.branchMissing');
    return result.message || t('gitEnv.worktreeFailed');
  };

  const handleCreate = async () => {
    if (!canSubmit) return;
    setBusy(true);
    try {
      const result = await gitCreateWorktree(dir, { name: trimmed, base: base || null, agentId });
      if (!result.httpOk || !result.ok) {
        addToast?.(describeFailure(result), 'error');
        return;
      }
      setName('');
      onClose();
      await onCreated({ path: result.path || '', branch: result.branch || '' });
    } finally {
      setBusy(false);
    }
  };

  /** 描述句里内嵌落地路径片段：整句仍由一个 locale key 承载，语序不拆 */
  const renderDesc = () => {
    // 落地根未知（worktree list 取不到）时退回不带路径的整句，不留空括号
    if (!worktreesRoot) return t('gitEnv.worktreeDescNoRoot');
    const template = t('gitEnv.worktreeDesc', { root: ROOT_SLOT });
    const index = template.indexOf(ROOT_SLOT);
    if (index < 0) return t('gitEnv.worktreeDesc', { root: worktreesRoot });
    return (
      <>
        {template.slice(0, index)}
        <code className={styles.pathChip}>{worktreesRoot}</code>
        {template.slice(index + ROOT_SLOT.length)}
      </>
    );
  };

  return (
    <Overlay
      scope="inline"
      open={open}
      onClose={onClose}
      backdrop="blur"
      className={styles.modal}
      initialFocusRef={nameRef}
      disableContainerAnimation
    >
      <div className={styles.header}>
        <h2 className={styles.title}>{t('gitEnv.worktreeTitle')}</h2>
        <button className={styles.closeBtn} onClick={onClose} aria-label={t('common.close')}>×</button>
      </div>
      <div className={styles.body}>
        <p className={styles.desc} data-testid="git-worktree-desc">{renderDesc()}</p>

        <label className={styles.field}>
          <span className={styles.label}>{t('gitEnv.worktreeNameLabel')}</span>
          <input
            ref={nameRef}
            className={styles.input}
            data-testid="git-worktree-name"
            value={name}
            spellCheck={false}
            placeholder={t('gitEnv.worktreeNamePlaceholder')}
            disabled={busy}
            onChange={e => setName(e.target.value)}
          />
        </label>
        {showNameHint && (
          <div className={styles.hint} data-testid="git-worktree-name-hint">
            {t('gitEnv.invalidWorktreeName')}
          </div>
        )}

        <label className={styles.field}>
          <span className={styles.label}>{t('gitEnv.worktreeBaseLabel')}</span>
          <select
            className={styles.select}
            data-testid="git-worktree-base"
            value={base}
            disabled={busy || baseOptions.length === 0}
            onChange={e => setBaseOverride(e.target.value)}
          >
            {baseOptions.length === 0 && <option value="">{t('gitEnv.worktreeBaseHead')}</option>}
            {baseOptions.map(option => <option key={option} value={option}>{option}</option>)}
          </select>
        </label>

        <div className={styles.actions}>
          <button type="button" className={styles.secondaryBtn} onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button
            type="button"
            className={styles.primaryBtn}
            data-testid="git-worktree-create"
            disabled={!canSubmit}
            onClick={() => void handleCreate()}
          >
            {busy ? t('gitEnv.worktreeCreating') : t('gitEnv.worktreeCreate')}
          </button>
        </div>
      </div>
    </Overlay>
  );
}
