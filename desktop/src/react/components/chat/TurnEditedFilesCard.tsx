/**
 * TurnEditedFilesCard — 助手消息下方的「本轮已编辑文件」卡
 *
 * 数据来源分工：
 *   文件清单  本条消息工具调用的 details.fileChange（edit/write/ast_edit 成功后
 *            都会登记；同一路径多次编辑取最后一次，天然是最终累计口径）。
 *   +N −N    优先取当前工作区 Git status 的逐文件统计（Git 状态按「本会话工作区」
 *            取：会话 cwd 优先、desk 字段兜底，与右侧「环境信息」卡同一 API），
 *            匹配不上（非 Git 仓库 / 路径对不上 / 新建文件）时退回调用自带统计。
 *   查看更改  复用 GitChangesModal，只列本轮命中的文件（含逐文件 diff / 暂存 / 回退）。
 *   撤销      git discard 本轮可回退（已跟踪）的文件，带确认弹窗。
 *   审核      向本会话发送一条固定的审核请求消息，让助手复查本轮改动。
 */
import { memo, useCallback, useEffect, useMemo, useState } from 'react';
import { useStore } from '../../stores';
import { ConfirmDialog } from '../../ui';
import type { ComposerSendBundle, ContentBlock } from '../../stores/chat-types';
import {
  fetchGitStatus,
  gitDiscard,
  type GitFileChange,
  type GitStatus,
} from '../../utils/git-env-api';
import { GitChangesModal } from '../runtime/GitChangesModal';
import {
  defaultComposerSendFlowDeps,
  sendWithLease,
  tryAcquireSendLease,
} from '../../services/composer-send-coordinator';
import { toolPatchStats } from '../../../../../shared/tool-presentation.ts';
import styles from './TurnEditedFilesCard.module.css';

/** 未展开时最多显示的文件行数（与设计稿一致：先露 3 行，其余折叠） */
const VISIBLE_ROWS = 3;

interface FileChangeDetail {
  path?: unknown;
  added?: unknown;
  removed?: unknown;
  patch?: unknown;
}

export interface TurnEditedFile {
  path: string;
  /** null = 调用记录里没有可靠的行数（补丁被省略/deferred），交给 Git 匹配兜底 */
  added: number | null;
  removed: number | null;
}

/** 从消息块里收集本轮编辑过的文件；同一路径多次编辑只留最后一次 */
export function collectTurnEditedFiles(blocks: readonly ContentBlock[]): TurnEditedFile[] {
  const byPath = new Map<string, TurnEditedFile>();
  for (const block of blocks) {
    if (block.type !== 'tool_group') continue;
    for (const tool of block.tools) {
      if (!tool.done || tool.success === false) continue;
      const change = tool.details?.fileChange as FileChangeDetail | undefined;
      const path = typeof change?.path === 'string' ? change.path.trim() : '';
      if (!path) continue;
      let added = typeof change?.added === 'number' ? change.added : null;
      let removed = typeof change?.removed === 'number' ? change.removed : null;
      if ((added === null || removed === null) && typeof change?.patch === 'string') {
        const stats = toolPatchStats(change.patch);
        if (stats) {
          added = stats.added;
          removed = stats.removed;
        }
      }
      byPath.set(path, { path, added, removed });
    }
  }
  return [...byPath.values()];
}

function normalizePath(path: string): string {
  return path.replace(/\\/g, '/').replace(/^\.\//, '');
}

/** 编辑路径（可能绝对/相对）与 Git 仓库相对路径按后缀对齐 */
export function matchGitFileForTest(path: string, files: readonly GitFileChange[]): GitFileChange | null {
  const p = normalizePath(path);
  for (const file of files) {
    const g = normalizePath(file.path);
    if (p === g || p.endsWith(`/${g}`) || g.endsWith(`/${p}`)) return file;
  }
  return null;
}

const matchGitFile = matchGitFileForTest;

function fmt(n: number): string {
  return n.toLocaleString('en-US');
}

function DiffBadgeIcon() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" />
      <polyline points="14 3 14 8 19 8" />
      <line x1="8.5" y1="13.5" x2="12.5" y2="13.5" />
      <line x1="10.5" y1="11.5" x2="10.5" y2="15.5" />
      <line x1="8.5" y1="17.5" x2="12.5" y2="17.5" />
    </svg>
  );
}

function ArrowUpRightIcon() {
  return (
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <line x1="7" y1="17" x2="17" y2="7" />
      <polyline points="8 7 17 7 17 16" />
    </svg>
  );
}

function UndoIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="9 14 4 9 9 4" />
      <path d="M4 9h10a6 6 0 0 1 6 6v1" />
    </svg>
  );
}

function ChevronDownIcon({ open }: { open: boolean }) {
  return (
    <svg className={styles.chevron} data-open={open || undefined} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="6 9 12 15 18 9" />
    </svg>
  );
}

interface Props {
  sessionPath: string;
  blocks: readonly ContentBlock[];
  isStreaming: boolean;
  readOnly: boolean;
}

export const TurnEditedFilesCard = memo(function TurnEditedFilesCard({
  sessionPath,
  blocks,
  isStreaming,
  readOnly,
}: Props) {
  const t = window.t ?? ((key: string, vars?: Record<string, string | number>) => key);
  const addToast = useStore(s => s.addToast);
  const editedFiles = useMemo(() => collectTurnEditedFiles(blocks), [blocks]);

  // Git 操作必须落在「本会话的工作区」上：会话 cwd 是 agent 实际编辑文件的目录，
  // 优先于 desk 字段（desk 可能开着别的目录，undo 会伤及无关仓库）。
  // mount 工作台的 deskBasePath 是 'studio:*' 键，不能当目录传给 git。
  const sessionCwd = useStore(s => s.sessions.find(item => item.path === sessionPath)?.cwd || null);
  const deskNativeRoot = useStore(s => s.deskWorkspaceNativeRoot);
  const deskBasePath = useStore(s => s.deskBasePath);
  const dir = useMemo(() => (
    [sessionCwd, deskNativeRoot, deskBasePath]
      .find((p): p is string => typeof p === 'string' && !!p.trim() && !p.startsWith('studio:')) || null
  ), [sessionCwd, deskNativeRoot, deskBasePath]);
  const isActiveSession = useStore(s => s.currentSessionPath === sessionPath);
  const currentSessionId = useStore(s => s.currentSessionId);
  const currentAgentId = useStore(s => s.currentAgentId);

  const [status, setStatus] = useState<GitStatus | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [changesOpen, setChangesOpen] = useState(false);
  const [confirmUndo, setConfirmUndo] = useState(false);
  const [undoBusy, setUndoBusy] = useState(false);
  const [reviewBusy, setReviewBusy] = useState(false);

  const refresh = useCallback(async (): Promise<GitStatus | null> => {
    if (!dir) return null;
    try {
      const next = await fetchGitStatus(dir, currentAgentId);
      setStatus(next);
      return next;
    } catch {
      setStatus(null);
      return null;
    }
  }, [dir, currentAgentId]);

  // 挂载/文件集变化时取一次 Git 状态；拿不到就整体退回调用自带行数
  useEffect(() => {
    if (dir && editedFiles.length > 0) void refresh();
  }, [dir, editedFiles.length, refresh]);

  const matches = useMemo(() => {
    if (!status?.isRepo) return null;
    return editedFiles.map(file => ({ file, git: matchGitFile(file.path, status.files) }));
  }, [editedFiles, status]);

  const matchedGitFiles = useMemo(
    () => (matches ?? []).map(m => m.git).filter((git): git is GitFileChange => !!git),
    [matches],
  );
  const discardablePaths = useMemo(
    () => (matches ?? [])
      .filter(m => m.git && m.git.state !== 'untracked')
      .map(m => normalizePath(m.git!.path)),
    [matches],
  );

  if (editedFiles.length === 0) return null;

  // 禁用原因要如实标注：工作区不是 Git 仓库时，「撤销/查看更改」不可用与
  // 「文件都是新建」是两回事，tooltip 不能混用同一条解释。
  const workspaceIsNotGitRepo = !!status && !status.isRepo;
  const viewUnavailableTitle = workspaceIsNotGitRepo
    ? t('chat.editedFiles.noGitRepo')
    : t('chat.editedFiles.viewUnavailable');
  const undoUnavailableTitle = workspaceIsNotGitRepo
    ? t('chat.editedFiles.noGitRepo')
    : t('chat.editedFiles.undoUnavailable');

  const canViewChanges = !!dir && matchedGitFiles.length > 0;
  const canUndo = !readOnly && !isStreaming && discardablePaths.length > 0;
  const canReview = !readOnly && !isStreaming && isActiveSession && !!currentSessionId;

  const visibleCount = expanded ? editedFiles.length : Math.min(VISIBLE_ROWS, editedFiles.length);
  const hiddenCount = editedFiles.length - visibleCount;

  const rowStats = (file: TurnEditedFile): { added: number | null; removed: number | null } => {
    const git = matches?.find(m => m.file.path === file.path)?.git ?? null;
    if (git && git.state !== 'untracked') return { added: git.additions, removed: git.deletions };
    return { added: file.added, removed: file.removed };
  };

  const enqueueReviewFallback = (text: string, bundle: ComposerSendBundle) => {
    useStore.getState().enqueueQueuedTurnInput(sessionPath, {
      id: `edited-files-review-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      sessionPath,
      text,
      createdAt: Date.now(),
      snapshotVersion: 1,
      status: 'ready',
      bundle,
    });
    addToast(t('chat.editedFiles.reviewQueued'), 'info');
  };

  const handleReview = async () => {
    if (!canReview || reviewBusy) return;
    setReviewBusy(true);
    const text = t('chat.editedFiles.reviewPrompt', {
      files: editedFiles.map(file => file.path).join('\n'),
    });
    const bundle: ComposerSendBundle = {
      type: 'prompt',
      sessionRef: { sessionId: currentSessionId!, sessionPath, agentId: currentAgentId },
      text,
      skills: [],
      fileRefs: [],
      sessionRefs: [],
      agentMentions: [],
      inputFiles: [],
      knowledgeRefs: null,
      docContextAttached: false,
      doc: null,
      quotes: [],
      uiContext: null,
    };
    try {
      const acq = tryAcquireSendLease({
        identity: { kind: 'session', sessionId: currentSessionId!, sessionPath, agentId: currentAgentId },
        bundle,
      });
      if (acq.ok) {
        const result = await sendWithLease(acq.leaseId, defaultComposerSendFlowDeps());
        if (result.kind === 'transport_submitted') {
          addToast(t('chat.editedFiles.reviewSent'), 'success');
          return;
        }
        // 投递结果不可证时不能入队重发（可能双发）；其余可重试失败走队列兜底
        if (result.kind !== 'delivery_unknown' && result.retryable) {
          enqueueReviewFallback(text, bundle);
          return;
        }
        addToast(t('chat.editedFiles.reviewFailed'), 'error');
        return;
      }
      enqueueReviewFallback(text, bundle);
    } finally {
      setReviewBusy(false);
    }
  };

  const handleUndo = async () => {
    if (!canUndo || undoBusy) return;
    setUndoBusy(true);
    setConfirmUndo(false);
    try {
      const result = await gitDiscard(dir!, { paths: discardablePaths, agentId: currentAgentId });
      if (!result.httpOk || !result.ok) {
        addToast(result.message || result.error || t('chat.editedFiles.undoFailed'), 'error');
        return;
      }
      addToast(t('chat.editedFiles.undoDone', { count: discardablePaths.length }), 'success');
      await refresh();
    } catch {
      addToast(t('chat.editedFiles.undoFailed'), 'error');
    } finally {
      setUndoBusy(false);
    }
  };

  return (
    <section className={styles.card} data-testid="turn-edited-files-card">
      <header className={styles.head}>
        <span className={styles.icon} aria-hidden="true">
          <DiffBadgeIcon />
        </span>
        <div className={styles.headText}>
          <span className={styles.title}>
            {t('chat.editedFiles.title', { count: editedFiles.length })}
          </span>
          <button
            type="button"
            className={styles.viewLink}
            disabled={!canViewChanges}
            title={canViewChanges ? undefined : viewUnavailableTitle}
            onClick={() => setChangesOpen(true)}
          >
            {t('chat.editedFiles.viewChanges')}
            <ArrowUpRightIcon />
          </button>
        </div>
        {!readOnly && (
          <div className={styles.headActions}>
            <button
              type="button"
              className={styles.ghostBtn}
              disabled={!canUndo || undoBusy}
              title={canUndo ? undefined : undoUnavailableTitle}
              onClick={() => setConfirmUndo(true)}
            >
              <UndoIcon />
              {undoBusy ? '…' : t('chat.editedFiles.undo')}
            </button>
            <button
              type="button"
              className={styles.outlineBtn}
              disabled={!canReview || reviewBusy}
              title={canReview ? undefined : t('chat.editedFiles.reviewUnavailable')}
              onClick={() => void handleReview()}
            >
              {reviewBusy ? '…' : t('chat.editedFiles.review')}
            </button>
          </div>
        )}
      </header>

      <ul className={styles.fileList}>
        {editedFiles.slice(0, visibleCount).map((file) => {
          const stats = rowStats(file);
          const hasStats = stats.added !== null || stats.removed !== null;
          return (
            <li key={file.path} className={styles.fileRow} title={file.path}>
              <span className={styles.filePath}>{file.path}</span>
              {hasStats && (
                <span className={styles.fileStats}>
                  <span className={styles.added}>+{fmt(stats.added ?? 0)}</span>
                  <span className={styles.deleted}>-{fmt(stats.removed ?? 0)}</span>
                </span>
              )}
            </li>
          );
        })}
      </ul>

      {editedFiles.length > VISIBLE_ROWS && (
        <button
          type="button"
          className={styles.moreToggle}
          data-expanded={expanded || undefined}
          onClick={() => setExpanded(v => !v)}
        >
          {expanded
            ? t('chat.editedFiles.showLess')
            : t('chat.editedFiles.showMore', { count: hiddenCount })}
          <ChevronDownIcon open={expanded} />
        </button>
      )}

      {canViewChanges && (
        <GitChangesModal
          open={changesOpen}
          onClose={() => setChangesOpen(false)}
          dir={dir!}
          files={matchedGitFiles}
          agentId={currentAgentId}
          refresh={refresh}
        />
      )}

      <ConfirmDialog
        open={confirmUndo}
        scope="inline"
        title={t('chat.editedFiles.undoConfirmTitle')}
        confirmLabel={t('chat.editedFiles.undoConfirmOk')}
        cancelLabel={t('common.cancel')}
        confirmTone="danger"
        busy={undoBusy}
        onConfirm={() => void handleUndo()}
        onCancel={() => setConfirmUndo(false)}
      >
        <div data-testid="turn-edited-files-undo-body">
          {t('chat.editedFiles.undoConfirmBody', { count: discardablePaths.length, list: discardablePaths.join('\n') })}
        </div>
      </ConfirmDialog>
    </section>
  );
});
