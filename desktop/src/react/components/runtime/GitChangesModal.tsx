/**
 * GitChangesModal — 变更文件列表弹窗（环境信息卡·变更行入口）
 *
 * 每行 = 文件名（超长省略号截断）+ 该文件 +N -N 增删；点击行内展开
 * 行级 diff（懒加载、会话内缓存）。diff 为 unified patch 逐行着色，
 * 复用文件历史弹窗的 add/remove/same 色值惯例。
 */
import { useEffect, useState } from 'react';
import { Overlay, Tooltip } from '../../ui';
import { fetchGitFileDiff, type GitFileChange, type GitFileDiff } from '../../utils/git-env-api';
import { parseUnifiedPatch } from '../../utils/unified-diff';
import styles from './GitChangesModal.module.css';

const MAX_RENDER_LINES = 1500;

type DiffCache = Record<string, GitFileDiff | 'error'>;

function fmt(n: number): string {
  return n.toLocaleString('en-US');
}

interface GitChangesModalProps {
  open: boolean;
  onClose: () => void;
  dir: string;
  files: GitFileChange[];
}

export function GitChangesModal({ open, onClose, dir, files }: GitChangesModalProps) {
  const t = window.t ?? ((p: string) => p);
  const [expandedPath, setExpandedPath] = useState<string | null>(null);
  const [loadingPath, setLoadingPath] = useState<string | null>(null);
  const [diffs, setDiffs] = useState<DiffCache>({});

  // 关闭重置展开态；diff 缓存保留到目录变化（组件由卡片持有，dir 变化即重挂载链路之外）
  useEffect(() => {
    if (!open) {
      setExpandedPath(null);
      setLoadingPath(null);
    }
  }, [open]);

  useEffect(() => {
    setDiffs({});
    setExpandedPath(null);
  }, [dir]);

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

  return (
    <Overlay scope="inline" open={open} onClose={onClose} backdrop="blur" className={styles.modal} disableContainerAnimation>
      <div className={styles.header}>
        <h2 className={styles.title}>{t('gitEnv.changesTitle')}</h2>
        <button className={styles.closeBtn} onClick={onClose} aria-label="Close">×</button>
      </div>
      <div className={styles.body}>
        {files.length === 0 ? (
          <div className={styles.empty}>{t('gitEnv.noChanges')}</div>
        ) : (
          files.map(file => {
            const expanded = expandedPath === file.path;
            const diff = diffs[file.path];
            return (
              <div key={file.path} className={styles.fileBlock}>
                <button
                  type="button"
                  className={styles.fileRow}
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
          })
        )}
      </div>
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
