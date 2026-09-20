/**
 * GitGraphPanel — Git图谱 弹窗（环境信息卡·Git图谱行入口）
 *
 * 提交或推送与变更文件合并后的单一界面（用户定稿的 Git图谱 方案）：
 *   头部   分支胶囊（点击切换分支）＋ 本地领先数
 *   消息框 提交信息输入（Enter = 执行主按钮动作），右上角四角星让模型生成
 *   主按钮 跟随分支状态：无可推送/待拉取时是「提交」（暂存区空则提交全部改动）；
 *          本地领先 → 同步更改 ↑n（推送）；远程领先 → 同步更改 ↓n（拉取）；
 *          双方都有 → 同步更改 n↓ m↑（先拉后推，拉取失败即停）
 *   菜单   提交 / 提交（修改）/ 提交和推送 / 提交和同步，点击即执行
 *   暂存的更改 只在有暂存文件时出现；标题仅「取消所有暂存修改」，
 *          行内仅「取消暂存修改」（最右侧贴状态字母）
 *   变更   标题「放弃所有更改 / 暂存所有更改」；行内「放弃更改 / 暂存更改」
 *   点击文件行展开该文件 diff（懒加载、会话内缓存）；放弃是破坏性操作走确认弹窗。
 * 储藏（stash）不再在这里：暂存语义改用 git index（git add / git restore --staged）。
 */
import { useEffect, useRef, useState } from 'react';
import { AnchoredPortal, ConfirmDialog, Overlay } from '../../ui';
import { useStore } from '../../stores';
import {
  fetchGitFileDiff,
  generateGitCommitMessage,
  gitAmend,
  gitCommit,
  gitDiscard,
  gitFetchRemote,
  gitPull,
  gitPush,
  gitStage,
  gitUnstage,
  type GitActionResult,
  type GitBranches,
  type GitFileChange,
  type GitFileDiff,
  type GitStatus,
} from '../../utils/git-env-api';
import { parseUnifiedPatch } from '../../utils/unified-diff';
import { GitBranchList } from './GitBranchList';
import branchStyles from './GitBranchList.module.css';
import styles from './GitGraphPanel.module.css';

const MAX_RENDER_LINES = 1500;

/** 每区首屏渲染上限：改动文件很多时先渲染这些，其余点「显示全部」再挂载（开面板不卡） */
const INITIAL_ROWS = 80;

type DiffCache = Record<string, GitFileDiff | 'error'>;

type BusyStep =
  | null | 'ai' | 'commit' | 'amend' | 'commit-push' | 'commit-sync'
  | 'push' | 'pull' | 'fetch' | 'stage' | 'unstage' | 'discard';

type CommitScenario = 'commit' | 'ahead' | 'behind' | 'sync';

/** 变更性质字母：与 Git图谱 定稿一致（M/U/A/D） */
function statusLetter(file: GitFileChange): { letter: string; hintKey: string } {
  if (file.state === 'untracked') return { letter: 'U', hintKey: 'gitEnv.letterU' };
  if (file.state === 'deleted') return { letter: 'D', hintKey: 'gitEnv.letterD' };
  if (file.staged && file.state === 'added') return { letter: 'A', hintKey: 'gitEnv.letterA' };
  return { letter: 'M', hintKey: 'gitEnv.letterM' };
}

/** 文件类型小图标（文字形状 + 装饰色，按扩展名映射） */
function fileGlyph(path: string): { glyph: string; color: string } {
  const lower = path.toLowerCase();
  if (/(^|\/)\.z?codeignore$/.test(lower) || /(^|\/)\.gitignore$/.test(lower)) return { glyph: '@', color: '#8a8a8a' };
  if (lower.endsWith('.ts') || lower.endsWith('.tsx')) return { glyph: 'TS', color: '#519aba' };
  if (lower.endsWith('.js') || lower.endsWith('.cjs') || lower.endsWith('.mjs') || lower.endsWith('.jsx')) return { glyph: 'JS', color: '#b5a12b' };
  if (lower.endsWith('.json')) return { glyph: '{}', color: '#b58a2b' };
  if (lower.endsWith('.css') || lower.endsWith('.scss') || lower.endsWith('.less')) return { glyph: '#', color: '#a074c4' };
  if (lower.endsWith('.md')) return { glyph: 'M↓', color: '#519aba' };
  if (lower.endsWith('.patch') || lower.endsWith('.diff')) return { glyph: '±', color: '#4ec9b0' };
  return { glyph: '—', color: '#8a8a8a' };
}

function fmt(n: number): string {
  return n.toLocaleString('en-US');
}

interface GitGraphPanelProps {
  open: boolean;
  onClose: () => void;
  dir: string;
  status: GitStatus | null;
  branches: GitBranches | null;
  sessionPath: string | null;
  agentId: string | null;
  /** 操作后让整卡重新取数 */
  refresh: () => Promise<unknown>;
}

export function GitGraphPanel({
  open, onClose, dir, status, branches, sessionPath, agentId, refresh,
}: GitGraphPanelProps) {
  const t = window.t ?? ((p: string) => p);
  const addToast = useStore(s => s.addToast);
  const [message, setMessage] = useState('');
  const [busy, setBusy] = useState<BusyStep>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [branchMenuOpen, setBranchMenuOpen] = useState(false);
  const [expandedPath, setExpandedPath] = useState<string | null>(null);
  const [loadingPath, setLoadingPath] = useState<string | null>(null);
  const [diffs, setDiffs] = useState<DiffCache>({});
  const [stagedCollapsed, setStagedCollapsed] = useState(false);
  const [changesCollapsed, setChangesCollapsed] = useState(false);
  const [showAllStaged, setShowAllStaged] = useState(false);
  const [showAllChanges, setShowAllChanges] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState<string[] | null>(null);
  const menuAnchorRef = useRef<HTMLDivElement>(null);
  const branchAnchorRef = useRef<HTMLButtonElement>(null);

  const isRepo = status?.isRepo ?? false;
  const files = status?.files ?? [];
  const stagedFiles = files.filter(f => f.staged);
  const unstagedFiles = files.filter(f => !f.staged);
  const stagedEmpty = stagedFiles.length === 0;
  const ahead = status?.ahead ?? 0;
  const behind = status?.behind ?? 0;
  const hasChanges = files.length > 0;

  // 主按钮形态：有领先/落后就走同步，否则是提交
  const scenario: CommitScenario = !isRepo
    ? 'commit'
    : behind > 0 && ahead > 0 ? 'sync'
      : ahead > 0 ? 'ahead'
        : behind > 0 ? 'behind'
          : 'commit';

  const branchLabel = status?.isRepo
    ? (status.detached
        ? t('gitEnv.detachedHead', { name: status.currentBranch ?? '' })
        : status.currentBranch ?? '—')
    : '—';

  useEffect(() => {
    if (!open) {
      setMessage('');
      setBusy(null);
      setMenuOpen(false);
      setBranchMenuOpen(false);
      setExpandedPath(null);
      setLoadingPath(null);
      setStagedCollapsed(false);
      setChangesCollapsed(false);
      setShowAllStaged(false);
      setShowAllChanges(false);
      setConfirmDiscard(null);
    }
  }, [open]);

  useEffect(() => {
    setDiffs({});
    setExpandedPath(null);
  }, [dir]);

  /** 项目自带的提交/推送门禁（lefthook 等）失败时，把常见检查报错翻译成人话 */
  const translateCheckFailure = (detail: string): string | null => {
    const eof = detail.match(/(.+?):(\d+): new blank line at EOF/);
    if (eof) return t('gitEnv.checkBlockEof', { file: eof[1], line: eof[2] });
    const trailing = detail.match(/(.+?):(\d+): trailing whitespace/);
    if (trailing) return t('gitEnv.checkBlockWhitespace', { file: trailing[1], line: trailing[2] });
    if (/failed to push some refs/.test(detail)) return t('gitEnv.checkBlockPush');
    if (/lefthook|pre-commit hook|pre-push hook|husky/i.test(detail)) return t('gitEnv.checkBlockHook');
    return null;
  };

  const describeFailure = (result: GitActionResult, fallback: string): string => {
    if (result.code === 'nothing_staged') return t('gitEnv.nothingStaged');
    if (result.code === 'nothing_to_commit') return t('gitEnv.nothingToCommit');
    if (result.code === 'nothing_to_push') return t('gitEnv.nothingToPush');
    if (result.code === 'no_remote') return t('gitEnv.noRemote');
    if (result.code === 'no_upstream') return t('gitEnv.noUpstream');
    if (result.code === 'diverged') return t('gitEnv.pullDiverged');
    if (result.code === 'local_changes') return t('gitEnv.pullLocalChanges');
    if (result.code === 'stage_failed') return t('gitEnv.stageFailed');
    if (result.code === 'unstage_failed') return t('gitEnv.unstageFailed');
    if (result.code === 'fetch_failed') return t('gitEnv.fetchFailed');
    if (result.code === 'nothing_to_discard') return t('gitEnv.discardNothing');
    const detail = result.message || result.error;
    if (detail) {
      const friendly = translateCheckFailure(detail);
      if (friendly) return friendly;
      return `${t('gitEnv.operationFailed')}: ${detail.length > 240 ? `${detail.slice(0, 240)}…` : detail}`;
    }
    return fallback;
  };

  /** 提交信息来源：输入框优先，留空走 AI 生成并回填（上下文跟随本次提交范围） */
  const obtainMessage = async (includeUnstaged: boolean): Promise<string | null> => {
    const trimmed = message.trim();
    if (trimmed) return trimmed;
    setBusy('ai');
    try {
      const ai = await generateGitCommitMessage(dir, { includeUnstaged, sessionPath, agentId });
      if (!ai.httpOk || !ai.message) {
        addToast?.(ai.error || t('gitEnv.aiFailed'), 'error');
        return null;
      }
      setMessage(ai.message);
      return ai.message;
    } finally {
      setBusy(null);
    }
  };

  /** 四角星按钮：只生成并回填提交信息，不提交 */
  const handleGenerate = async () => {
    if (busy) return;
    setBusy('ai');
    try {
      const ai = await generateGitCommitMessage(dir, { includeUnstaged: stagedEmpty, sessionPath, agentId });
      if (!ai.httpOk || !ai.message) {
        addToast?.(ai.error || t('gitEnv.aiFailed'), 'error');
        return;
      }
      setMessage(ai.message);
    } finally {
      setBusy(null);
    }
  };

  /** 提交：暂存区有内容只提交暂存，空则提交全部改动 */
  const runCommit = async (): Promise<boolean> => {
    const includeUnstaged = stagedEmpty;
    const msg = await obtainMessage(includeUnstaged);
    if (msg == null) return false;
    setBusy('commit');
    try {
      const result = await gitCommit(dir, { message: msg, includeUnstaged, agentId });
      if (!result.httpOk || !result.ok) {
        addToast?.(describeFailure(result, t('gitEnv.operationFailed')), 'error');
        return false;
      }
      // 清空输入框 + 成功提示：明确告诉用户「提交已经完成」
      setMessage('');
      addToast?.(t('gitEnv.commitDone'), 'success');
      return true;
    } finally {
      setBusy(null);
    }
  };

  const runPush = async (silentNothingToPush: boolean): Promise<boolean> => {
    const result = await gitPush(dir, agentId);
    if (result.httpOk && result.ok) {
      addToast?.(t('gitEnv.pushDone'), 'success');
      return true;
    }
    if (silentNothingToPush && result.code === 'nothing_to_push') return true;
    addToast?.(describeFailure(result, t('gitEnv.operationFailed')), 'error');
    return false;
  };

  /** 拉取；已是最新也算就绪（返回 true 供「提交和同步」继续往下走） */
  const runPull = async (): Promise<boolean> => {
    const result = await gitPull(dir, agentId);
    if (result.httpOk && result.ok) {
      addToast?.(t('gitEnv.pullDone', { count: result.pulled ?? 0 }), 'success');
      return true;
    }
    if (result.code === 'already_up_to_date') {
      addToast?.(t('gitEnv.pullUpToDate'), 'success');
      return true;
    }
    addToast?.(describeFailure(result, t('gitEnv.operationFailed')), 'error');
    return false;
  };

  /** 主按钮：提交 / 推送（↑n）/ 拉取（↓n）/ 先拉后推（n↓ m↑） */
  const runMainAction = async (): Promise<void> => {
    if (busy || !isRepo) return;
    if (scenario === 'ahead') {
      // 先直接推送；推不动（远程先走了新提交，本地落后数未知）就转入「先拉后推」，
      // 拉取会 fetch 回真实状态并把按钮纠正成 n↓ m↑；分叉时给人话指引而非原始报错
      setBusy('push');
      let pushed = false;
      let pushRejected = false;
      try {
        const result = await gitPush(dir, agentId);
        if (result.httpOk && result.ok) {
          addToast?.(t('gitEnv.pushDone'), 'success');
          pushed = true;
        } else if (result.code === 'push_failed') {
          pushRejected = true;
        } else {
          addToast?.(describeFailure(result, t('gitEnv.operationFailed')), 'error');
        }
      } finally {
        setBusy(null);
      }
      if (pushed) {
        await refresh();
        return;
      }
      if (pushRejected) {
        setBusy('pull');
        let pulled = false;
        try { pulled = await runPull(); } finally { setBusy(null); }
        if (pulled) {
          setBusy('push');
          try { await runPush(true); } finally { setBusy(null); }
        }
      }
      await refresh();
      return;
    }
    if (scenario === 'behind') {
      setBusy('pull');
      try { await runPull(); } finally { setBusy(null); }
      await refresh();
      return;
    }
    if (scenario === 'sync') {
      setBusy('pull');
      let pulled = false;
      try { pulled = await runPull(); } finally { setBusy(null); }
      if (!pulled) return;
      setBusy('push');
      try { await runPush(true); } finally { setBusy(null); }
      await refresh();
      return;
    }
    // scenario === 'commit'
    if (!hasChanges) return;
    if (!(await runCommit())) return;
    await refresh();
  };

  /** 菜单：提交（修改）——暂存区空先把全部改动收进暂存，与「提交」的语义对齐 */
  const handleAmend = async () => {
    if (busy || !isRepo) return;
    setBusy('amend');
    try {
      if (stagedEmpty) {
        const stagedAll = await gitStage(dir, { agentId });
        if (!stagedAll.httpOk || !stagedAll.ok) {
          addToast?.(describeFailure(stagedAll, t('gitEnv.stageFailed')), 'error');
          return;
        }
      }
      const result = await gitAmend(dir, { message: message.trim() || null, agentId });
      if (!result.httpOk || !result.ok) {
        addToast?.(describeFailure(result, t('gitEnv.operationFailed')), 'error');
        return;
      }
      addToast?.(t('gitEnv.amendDone'), 'success');
      setMessage('');
      await refresh();
    } finally {
      setBusy(null);
    }
  };

  const handleCommitPush = async () => {
    if (busy || !isRepo) return;
    setBusy('commit-push');
    try {
      if (hasChanges) {
        if (!(await runCommit())) return;
        await refresh();
      }
      if (!(await runPush(true))) return;
      await refresh();
    } finally {
      setBusy(null);
    }
  };

  const handleCommitSync = async () => {
    if (busy || !isRepo) return;
    setBusy('commit-sync');
    try {
      if (!(await runPull())) return;
      await refresh();
      if (hasChanges) {
        if (!(await runCommit())) return;
        await refresh();
      }
      if (!(await runPush(true))) return;
      await refresh();
    } finally {
      setBusy(null);
    }
  };

  /** 刷新：有远程时先 fetch 把远程真实状态带回来，再重读本地状态 */
  const handleRefresh = async () => {
    if (busy) return;
    if (isRepo && status?.hasRemote) {
      setBusy('fetch');
      try {
        const result = await gitFetchRemote(dir, agentId);
        if (!result.httpOk || !result.ok) {
          addToast?.(describeFailure(result, t('gitEnv.fetchFailed')), 'error');
          return;
        }
      } finally {
        setBusy(null);
      }
    }
    await refresh();
    addToast?.(t('gitEnv.refreshDone'), 'success');
  };

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

  const stageOne = async (file: GitFileChange) => {
    if (busy) return;
    setBusy('stage');
    try {
      const result = await gitStage(dir, { paths: [file.path], agentId });
      if (!result.httpOk || !result.ok) {
        addToast?.(describeFailure(result, t('gitEnv.stageFailed')), 'error');
        return;
      }
      addToast?.(t('gitEnv.stageDone', { name: file.path }), 'success');
      await refresh();
    } finally {
      setBusy(null);
    }
  };

  const unstageOne = async (file: GitFileChange) => {
    if (busy) return;
    setBusy('unstage');
    try {
      const result = await gitUnstage(dir, { paths: [file.path], agentId });
      if (!result.httpOk || !result.ok) {
        addToast?.(describeFailure(result, t('gitEnv.unstageFailed')), 'error');
        return;
      }
      addToast?.(t('gitEnv.unstageDone', { name: file.path }), 'success');
      await refresh();
    } finally {
      setBusy(null);
    }
  };

  /** 回退是破坏性操作：单个与整体都先走确认弹窗（`[]` = 整个仓库） */
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

  const confirmDiscardNow = async () => {
    const paths = confirmDiscard;
    setConfirmDiscard(null);
    if (!paths) return;
    setBusy('discard');
    try {
      await runDiscard(paths.length > 0 ? paths : null);
    } finally {
      setBusy(null);
    }
  };

  const confirmPaths = confirmDiscard ?? [];
  const confirmWholeRepo = confirmPaths.length === 0;
  const frozen = busy != null;

  const renderRow = (file: GitFileChange, isStaged: boolean) => {
    const expanded = expandedPath === file.path;
    const diff = diffs[file.path];
    const loading = loadingPath === file.path;
    const glyph = fileGlyph(file.path);
    const segs = file.path.split('/');
    const name = segs[segs.length - 1] ?? file.path;
    const dirPath = segs.slice(0, -1).join('/');
    const { letter, hintKey } = statusLetter(file);
    return (
      <div key={file.path} className={styles.fileBlock} data-testid={`git-graph-file-${file.path}`}>
        <div className={styles.fileLine}>
          <span className={styles.fileGlyph} style={{ color: glyph.color }}>{glyph.glyph}</span>
          <button
            type="button"
            className={styles.fileMain}
            data-testid={`git-graph-file-toggle-${file.path}`}
            aria-expanded={expanded}
            onClick={() => void toggleFile(file)}
          >
            <span className={styles.fileName}>{name}</span>
            {dirPath && <span className={styles.filePath}>{dirPath}</span>}
          </button>
          <span className={styles.fileActs}>
            {isStaged ? (
              <button
                type="button"
                className={styles.fileAct}
                data-testid={`git-graph-row-unstage-${file.path}`}
                title={t('gitEnv.unstageOne')}
                disabled={frozen}
                onClick={() => void unstageOne(file)}
              >
                <MinusIcon />
              </button>
            ) : (
              <>
                <button
                  type="button"
                  className={`${styles.fileAct} ${styles.fileActDanger}`}
                  data-testid={`git-graph-row-discard-${file.path}`}
                  title={file.state === 'untracked'
                    ? t('gitEnv.discardUntrackedHint')
                    : t('gitEnv.discardOneHint')}
                  disabled={frozen}
                  onClick={() => setConfirmDiscard([file.path])}
                >
                  <Undo2Icon />
                </button>
                <button
                  type="button"
                  className={styles.fileAct}
                  data-testid={`git-graph-row-stage-${file.path}`}
                  title={t('gitEnv.stageOne')}
                  disabled={frozen}
                  onClick={() => void stageOne(file)}
                >
                  <PlusIcon />
                </button>
              </>
            )}
          </span>
          <span className={`${styles.fileState} ${styles[`state_${letter}`] ?? ''}`} title={t(hintKey)}>
            {letter}
          </span>
        </div>
        {expanded && (
          <div className={styles.diffPane} data-testid={`git-diff-${file.path}`}>
            {loading && <div className={styles.diffNote}>…</div>}
            {!loading && diff === 'error' && <div className={styles.diffNote}>{t('gitEnv.diffUnavailable')}</div>}
            {!loading && diff && diff !== 'error' && (diff.binary
              ? <div className={styles.diffNote}>{t('gitEnv.diffBinary')}</div>
              : <DiffBody patch={diff.patch} truncatedNote={t('gitEnv.diffTruncated')} emptyNote={t('gitEnv.diffUnavailable')} />)}
          </div>
        )}
      </div>
    );
  };

  if (!open) return null;

  const mainTitle = scenario === 'ahead'
    ? t('gitEnv.syncPushHint', { n: fmt(ahead) })
    : scenario === 'behind'
      ? t('gitEnv.syncPullHint', { n: fmt(behind) })
      : scenario === 'sync'
        ? t('gitEnv.syncBothHint', { n: fmt(behind), m: fmt(ahead) })
        : t('gitEnv.btnCommit');

  const mainDisabled = frozen || !isRepo
    || (scenario === 'commit' && !hasChanges)
    || (scenario === 'ahead' && ahead === 0)
    || (scenario === 'behind' && behind === 0);

  return (
    <Overlay scope="inline" open={open} onClose={onClose} backdrop="blur" className={styles.modal} disableContainerAnimation>
      <div className={styles.header}>
        <h2 className={styles.title}>{t('gitEnv.graphTitle')}</h2>
        <div className={styles.headerActions}>
          <button
            type="button"
            className={styles.refreshBtn}
            data-testid="git-graph-refresh"
            title={status?.hasRemote ? t('gitEnv.refreshInfo') : t('gitEnv.refreshLocalOnly')}
            disabled={frozen}
            onClick={() => void handleRefresh()}
          >
            <RefreshIcon spin={busy === 'fetch'} />
          </button>
          <button className={styles.closeBtn} onClick={onClose} aria-label="Close">×</button>
        </div>
      </div>
      <div className={styles.body}>
        <button
          type="button"
          ref={branchAnchorRef}
          className={styles.branchButton}
          data-testid="git-graph-branch"
          title={t('gitEnv.branchesTitle')}
          disabled={!isRepo}
          onClick={() => setBranchMenuOpen(v => !v)}
        >
          <BranchIcon />
          <span className={styles.branchName}>{branchLabel}</span>
          {ahead > 0 && (
            <span className={styles.aheadBadge} title={t('gitEnv.aheadHint', { n: fmt(ahead) })}>
              ↑{fmt(ahead)}
            </span>
          )}
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </button>

        <div className={styles.messageWrap}>
          <textarea
            className={styles.messageInput}
            data-testid="git-graph-message"
            rows={2}
            value={message}
            onChange={e => setMessage(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void runMainAction();
              }
            }}
            placeholder={t('gitEnv.graphMessagePlaceholder', { branch: status?.currentBranch ?? '' })}
            disabled={frozen}
          />
          <button
            type="button"
            className={styles.sparkBtn}
            data-testid="git-graph-generate"
            title={t('gitEnv.genMessageHint')}
            disabled={frozen || !hasChanges}
            onClick={() => void handleGenerate()}
          >
            {busy === 'ai'
              ? <span className={styles.sparkBusy}>{t('gitEnv.aiGeneratingShort')}</span>
              : <SparkIcon />}
          </button>
        </div>

        <div className={styles.menuWrap} ref={menuAnchorRef}>
          <div className={styles.split} role="group">
            <button
              type="button"
              className={styles.splitMain}
              data-testid="git-graph-main-btn"
              title={mainTitle}
              disabled={mainDisabled}
              onClick={() => void runMainAction()}
            >
              {busy === 'ai'
                ? <span>{t('gitEnv.aiGenerating')}</span>
                : scenario === 'commit'
                  ? (<><CheckIcon /><span>{t('gitEnv.btnCommit')}</span></>)
                  : (<>
                    <SyncIcon spin={busy === 'push' || busy === 'pull' || busy === 'commit-sync' || busy === 'commit-push'} />
                    <span>{t('gitEnv.syncChanges')}</span>
                    {scenario === 'ahead' && <span className={styles.syncCount}>
                      {fmt(ahead)}<ArrowUpIcon />
                    </span>}
                    {scenario === 'behind' && <span className={styles.syncCount}>
                      {fmt(behind)}<ArrowDownIcon />
                    </span>}
                    {scenario === 'sync' && <span className={styles.syncCount}>
                      {fmt(behind)}<ArrowDownIcon /> {fmt(ahead)}<ArrowUpIcon />
                    </span>}
                  </>)}
            </button>
            <button
              type="button"
              className={styles.splitArrow}
              data-testid="git-graph-menu-btn"
              aria-label={t('gitEnv.commitTitle')}
              disabled={frozen || !isRepo}
              onClick={() => setMenuOpen(v => !v)}
            >
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <polyline points="6 9 12 15 18 9" />
              </svg>
            </button>
          </div>

          <AnchoredPortal
            open={menuOpen}
            anchorRef={menuAnchorRef}
            onClose={() => setMenuOpen(false)}
            role="dialog"
            className={`${styles.menu} runtime-capsule-anchored`}
            align="start"
            matchAnchorWidth
          >
            <button type="button" className={styles.menuItem} data-testid="git-graph-menu-commit" disabled={frozen || !hasChanges} onClick={() => { setMenuOpen(false); void (async () => { if (await runCommit()) await refresh(); })(); }}>
              <span>{t('gitEnv.btnCommit')}</span>
            </button>
            <button type="button" className={styles.menuItem} data-testid="git-graph-menu-amend" disabled={frozen || !hasChanges} onClick={() => { setMenuOpen(false); void handleAmend(); }}>
              <span>{t('gitEnv.amendBtn')}</span>
              <span className={styles.menuSub}>{t('gitEnv.amendHint')}</span>
            </button>
            <button type="button" className={styles.menuItem} data-testid="git-graph-menu-commit-push" disabled={frozen || !isRepo} onClick={() => { setMenuOpen(false); void handleCommitPush(); }}>
              <span>{t('gitEnv.btnCommitPush')}</span>
            </button>
            <button type="button" className={styles.menuItem} data-testid="git-graph-menu-commit-sync" disabled={frozen || !isRepo} onClick={() => { setMenuOpen(false); void handleCommitSync(); }}>
              <span>{t('gitEnv.commitSyncBtn')}</span>
              <span className={styles.menuSub}>{t('gitEnv.commitSyncHint')}</span>
            </button>
          </AnchoredPortal>

          {!status?.hasRemote && (
            <div className={styles.noRemoteNotice} data-testid="git-graph-no-remote">
              {t('gitEnv.noRemoteNotice')}
            </div>
          )}
        </div>

        <div className={styles.sections} data-testid="git-graph-sections">
          {hasChanges ? (
            <>
              {stagedFiles.length > 0 && (
                <>
                  <div
                    className={styles.secHead}
                    data-testid="git-graph-staged-head"
                    role="button"
                    aria-expanded={!stagedCollapsed}
                    onClick={() => setStagedCollapsed(v => !v)}
                  >
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{ transform: stagedCollapsed ? 'rotate(-90deg)' : undefined }}>
                      <polyline points="6 9 12 15 18 9" />
                    </svg>
                    <span className={styles.secTitle}>{t('gitEnv.stagedChanges')}</span>
                    <span className={styles.secTools}>
                      <button
                        type="button"
                        className={styles.secTool}
                        data-testid="git-unstage-all"
                        title={t('gitEnv.unstageAll')}
                        disabled={frozen}
                        onClick={(e) => {
                          e.stopPropagation();
                          void (async () => {
                            const result = await gitUnstage(dir, { agentId });
                            if (!result.httpOk || !result.ok) {
                              addToast?.(describeFailure(result, t('gitEnv.unstageFailed')), 'error');
                              return;
                            }
                            addToast?.(t('gitEnv.unstageAllDone'), 'success');
                            await refresh();
                          })();
                        }}
                      >
                        <MinusIcon />
                      </button>
                    </span>
                    <span className={styles.secBadge}>{fmt(stagedFiles.length)}</span>
                  </div>
                  {!stagedCollapsed && (
                    <div className={styles.fileList}>
                      {(showAllStaged ? stagedFiles : stagedFiles.slice(0, INITIAL_ROWS)).map(f => renderRow(f, true))}
                      {!showAllStaged && stagedFiles.length > INITIAL_ROWS && (
                        <button
                          type="button"
                          className={styles.showAllBtn}
                          onClick={(e) => {
                            e.stopPropagation();
                            setShowAllStaged(true);
                          }}
                        >
                          {t('gitEnv.graphShowAll', { n: fmt(stagedFiles.length) })}
                        </button>
                      )}
                    </div>
                  )}
                </>
              )}

              {unstagedFiles.length > 0 && (
                <>
                  <div
                    className={styles.secHead}
                    data-testid="git-graph-changes-head"
                    role="button"
                    aria-expanded={!changesCollapsed}
                    onClick={() => setChangesCollapsed(v => !v)}
                  >
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{ transform: changesCollapsed ? 'rotate(-90deg)' : undefined }}>
                      <polyline points="6 9 12 15 18 9" />
                    </svg>
                    <span className={styles.secTitle}>{t('gitEnv.changes')}</span>
                    <span className={styles.secTools}>
                      <button
                        type="button"
                        className={`${styles.secTool} ${styles.secToolDanger}`}
                        data-testid="git-discard-all"
                        title={t('gitEnv.discardAllTitle')}
                        disabled={frozen}
                        onClick={(e) => {
                          e.stopPropagation();
                          setConfirmDiscard([]);
                        }}
                      >
                        <Undo2Icon />
                      </button>
                      <button
                        type="button"
                        className={styles.secTool}
                        data-testid="git-stage-all"
                        title={t('gitEnv.stageAllTitle')}
                        disabled={frozen}
                        onClick={(e) => {
                          e.stopPropagation();
                          void (async () => {
                            const result = await gitStage(dir, { agentId });
                            if (!result.httpOk || !result.ok) {
                              addToast?.(describeFailure(result, t('gitEnv.stageFailed')), 'error');
                              return;
                            }
                            addToast?.(t('gitEnv.stageAllDone'), 'success');
                            await refresh();
                          })();
                        }}
                      >
                        <PlusIcon />
                      </button>
                    </span>
                    <span className={styles.secBadge}>{fmt(unstagedFiles.length)}</span>
                  </div>
                  {!changesCollapsed && (
                    <div className={styles.fileList}>
                      {(showAllChanges ? unstagedFiles : unstagedFiles.slice(0, INITIAL_ROWS)).map(f => renderRow(f, false))}
                      {!showAllChanges && unstagedFiles.length > INITIAL_ROWS && (
                        <button
                          type="button"
                          className={styles.showAllBtn}
                          onClick={(e) => {
                            e.stopPropagation();
                            setShowAllChanges(true);
                          }}
                        >
                          {t('gitEnv.graphShowAll', { n: fmt(unstagedFiles.length) })}
                        </button>
                      )}
                    </div>
                  )}
                </>
              )}
            </>
          ) : (
            <div className={styles.empty} data-testid="git-graph-clean">
              {t('gitEnv.graphCleanTree')}
            </div>
          )}
        </div>
      </div>

      {/* 破坏性回退的确认弹窗：作为悬浮层直接子节点（同 GitChangesModal 结构） */}
      <ConfirmDialog
        open={confirmDiscard != null}
        scope="inline"
        title={t('gitEnv.discardConfirmTitle')}
        confirmLabel={t('gitEnv.discardConfirmOk')}
        cancelLabel={t('common.cancel')}
        confirmTone="danger"
        busy={busy === 'discard'}
        onConfirm={() => void confirmDiscardNow()}
        onCancel={() => setConfirmDiscard(null)}
      >
        <div data-testid="git-discard-confirm-body">
          {confirmWholeRepo
            ? t('gitEnv.discardConfirmAll')
            : t('gitEnv.discardConfirmFiles', { count: confirmPaths.length, list: confirmPaths.join('\n') })}
        </div>
      </ConfirmDialog>

      <AnchoredPortal
        open={branchMenuOpen && isRepo}
        anchorRef={branchAnchorRef}
        onClose={() => setBranchMenuOpen(false)}
        role="dialog"
        className={`${branchStyles.menu} runtime-capsule-anchored`}
        align="start"
        minWidth={220}
      >
        <GitBranchList
          dir={dir}
          agentId={agentId}
          branches={branches}
          busy={frozen}
          testIdPrefix="git-graph-branch"
          onChanged={async () => {
            setBranchMenuOpen(false);
            await refresh();
          }}
        />
      </AnchoredPortal>
    </Overlay>
  );
}

// ────────────────────────── 小图标（lucide 形状，行内装饰） ──────────────────────────

function CheckIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

function SyncIcon({ spin }: { spin?: boolean }) {
  return (
    <svg className={spin ? styles.spin : undefined} width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="23 4 23 10 17 10" />
      <polyline points="1 20 1 14 7 14" />
      <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
    </svg>
  );
}

function ArrowUpIcon() {
  return (
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <line x1="12" y1="19" x2="12" y2="5" />
      <polyline points="6 11 12 5 18 11" />
    </svg>
  );
}

function ArrowDownIcon() {
  return (
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <line x1="12" y1="5" x2="12" y2="19" />
      <polyline points="18 13 12 19 6 13" />
    </svg>
  );
}

function PlusIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  );
}

function MinusIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  );
}

/** undo-2（lucide）：放弃更改 / 放弃所有更改 的定稿图标 */
function Undo2Icon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 14 4 9l5-5" />
      <path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11" />
    </svg>
  );
}

function SparkIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z" />
      <path d="M18.5 15.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z" />
    </svg>
  );
}

function BranchIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" aria-hidden="true">
      <circle cx="6" cy="6" r="2.6" />
      <circle cx="6" cy="18" r="2.6" />
      <circle cx="18" cy="8" r="2.6" />
      <path d="M6 8.6v6.8M17 10.5c0 3.4-4.5 3.9-8.6 4.3" />
    </svg>
  );
}

/** 刷新（rotate-cw）：拉取远程与本地最新信息 */
function RefreshIcon({ spin }: { spin?: boolean }) {
  return (
    <svg className={spin ? styles.spin : undefined} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="23 4 23 10 17 10" />
      <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
    </svg>
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
