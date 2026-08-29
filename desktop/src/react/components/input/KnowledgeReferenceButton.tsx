import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { useI18n } from '../../hooks/use-i18n';
import { useStore } from '../../stores';
import { selectKnowledgeRefsForSession } from '../../stores/knowledge-reference-slice';
import { listKnowledgeNotebooks, type KnowledgeNotebookDto } from '../knowledge/knowledge-api';
import styles from './InputArea.module.css';

type NotebookReadiness = 'empty' | 'failed' | 'processing' | 'pendingEmbedding' | 'pendingIngestion' | 'ready';

/** 菜单徽章只报最值得注意的一种状态（失败 > 摄入中 > 待嵌入 > 待摄入 > 就绪），与知识页 readinessSuffix 同一语义。 */
function notebookReadiness(notebook: KnowledgeNotebookDto): NotebookReadiness {
  if (notebook.sourceCount === 0) return 'empty';
  if (notebook.ingestion.failed > 0) return 'failed';
  if (notebook.ingestion.processing > 0) return 'processing';
  if (notebook.ingestion.pendingEmbedding > 0) return 'pendingEmbedding';
  // untracked：有源尚无摄入 job（导入进行中或历史数据未回填），不能显示就绪。
  if (notebook.ingestion.untracked > 0) return 'pendingIngestion';
  return 'ready';
}

function readinessLabel(notebook: KnowledgeNotebookDto, readiness: NotebookReadiness, t: (key: string, params?: Record<string, string | number>) => string): string {
  switch (readiness) {
    case 'failed': return t('knowledge.readinessFailed', { count: notebook.ingestion.failed });
    case 'processing': return t('knowledge.readinessProcessing', { count: notebook.ingestion.processing });
    case 'pendingEmbedding': return t('knowledge.readinessPendingEmbedding', { count: notebook.ingestion.pendingEmbedding });
    case 'pendingIngestion': return t('knowledge.statusPendingIngestion');
    case 'empty': return t('knowledge.sourceCount', { count: 0 });
    default: return t('knowledge.statusReady');
  }
}

/**
 * 输入框下方工具栏的「知识库」按钮：弹出菜单列出全部笔记本
 * （名称 + 源数 + 就绪徽章），点击 = 整体引用/取消引用该笔记本。
 * 引用按 session 隔离、持续生效直到手动取消，支持多选，菜单选择后不关闭。
 */
export const KnowledgeReferenceButton = memo(function KnowledgeReferenceButton({ sessionKey, disabled }: {
  /** 当前会话键（sessionPath；pending 新会话为 HOME_DRAFT_KEY；无可写会话时为 null，按钮禁用） */
  sessionKey: string | null;
  disabled?: boolean;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [notebooks, setNotebooks] = useState<KnowledgeNotebookDto[] | null>(null);
  const [loadState, setLoadState] = useState<'idle' | 'loading' | 'error'>('idle');
  const rootRef = useRef<HTMLDivElement>(null);
  const referencedIds = useStore(s => selectKnowledgeRefsForSession(s, sessionKey)?.notebookIds);
  const toggleKnowledgeNotebook = useStore(s => s.toggleKnowledgeNotebook);

  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onMouseDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('mousedown', onMouseDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open]);

  // 每次打开都重新拉取，保证源数/就绪徽章是最新的。
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoadState('loading');
    listKnowledgeNotebooks()
      .then((list) => {
        if (cancelled) return;
        setNotebooks(list);
        setLoadState('idle');
      })
      .catch(() => {
        if (cancelled) return;
        setNotebooks(null);
        setLoadState('error');
      });
    return () => { cancelled = true; };
  }, [open]);

  const handleToggle = useCallback((notebook: KnowledgeNotebookDto) => {
    if (!sessionKey) return;
    toggleKnowledgeNotebook(sessionKey, notebook.id, notebook.name);
  }, [sessionKey, toggleKnowledgeNotebook]);

  const active = (referencedIds?.length ?? 0) > 0;

  return (
    <div className={`${styles['thinking-selector']} ${styles['knowledge-ref-selector']}${open ? ` ${styles.open}` : ''}`} ref={rootRef}>
      <button
        type="button"
        className={`${styles['attach-btn']}${active ? ` ${styles['knowledge-ref-btn-active']}` : ''}`}
        title={t('input.knowledgeButton')}
        aria-label={t('input.knowledgeButton')}
        aria-expanded={open}
        disabled={disabled || !sessionKey}
        onClick={(e) => { e.stopPropagation(); setOpen(!open); }}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
          <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
          <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
        </svg>
      </button>
      {open && (
        <div className={styles['knowledge-ref-menu']} role="menu" aria-label={t('input.knowledgeButton')}>
          {loadState === 'loading' && (
            <div className={styles['knowledge-ref-menu-status']}>{t('knowledge.loading')}</div>
          )}
          {loadState === 'error' && (
            <div className={styles['knowledge-ref-menu-status']}>{t('knowledge.loadFailed')}</div>
          )}
          {loadState === 'idle' && notebooks && notebooks.length === 0 && (
            <div className={styles['knowledge-ref-menu-status']}>{t('knowledge.emptyNotebooks')}</div>
          )}
          {loadState === 'idle' && notebooks?.map((notebook) => {
            const checked = referencedIds?.includes(notebook.id) ?? false;
            const readiness = notebookReadiness(notebook);
            return (
              <button
                key={notebook.id}
                type="button"
                role="menuitemcheckbox"
                aria-checked={checked}
                className={`${styles['knowledge-ref-item']}${checked ? ` ${styles.checked}` : ''}`}
                onClick={() => handleToggle(notebook)}
              >
                <span className={styles['knowledge-ref-check']} aria-hidden="true">
                  {checked && (
                    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                  )}
                </span>
                <span className={styles['knowledge-ref-name']} title={notebook.name}>{notebook.name}</span>
                <span className={styles['knowledge-ref-meta']}>{t('knowledge.sourceCount', { count: notebook.sourceCount })}</span>
                <span className={`${styles['knowledge-ref-badge']} ${styles[`knowledge-ref-badge-${readiness}`] || ''}`}>
                  {readinessLabel(notebook, readiness, t)}
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
});
