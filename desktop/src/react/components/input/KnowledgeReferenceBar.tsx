import { memo, useEffect, useState } from 'react';
import { useI18n } from '../../hooks/use-i18n';
import { useStore } from '../../stores';
import { selectKnowledgeRefsForSession, type KnowledgeReferenceMode } from '../../stores/knowledge-reference-slice';
import { listKnowledgeNotebooks, type KnowledgeNotebookDto } from '../knowledge/knowledge-api';
import styles from './InputArea.module.css';

/**
 * 输入框上方的知识库引用条（类似附件条）：已引用笔记本 chip（× 可移除）
 * + 快速/详细回答模式切换。无引用时整体不渲染。
 *
 * 笔记本名称解析顺序：最新列表 → slice 名称缓存 → 原始 id（脏 id 兜底可见、可移除）。
 */
export const KnowledgeReferenceBar = memo(function KnowledgeReferenceBar({ sessionKey }: {
  sessionKey: string | null;
}) {
  const { t } = useI18n();
  const refs = useStore(s => selectKnowledgeRefsForSession(s, sessionKey));
  const removeKnowledgeNotebook = useStore(s => s.removeKnowledgeNotebook);
  const setKnowledgeReferenceMode = useStore(s => s.setKnowledgeReferenceMode);
  const [notebooks, setNotebooks] = useState<KnowledgeNotebookDto[] | null>(null);

  const ids = refs?.notebookIds;
  const idsKey = ids?.join('\n') ?? '';
  // 有引用时才拉列表解析名称；失败保持 null，chip 退回名称缓存/id。
  useEffect(() => {
    if (!idsKey) return;
    let cancelled = false;
    listKnowledgeNotebooks()
      .then((list) => { if (!cancelled) setNotebooks(list); })
      .catch(() => { if (!cancelled) setNotebooks(null); });
    return () => { cancelled = true; };
  }, [idsKey]);

  if (!sessionKey || !refs || refs.notebookIds.length === 0) return null;

  const nameOf = (id: string): string =>
    notebooks?.find(nb => nb.id === id)?.name ?? refs.notebookNames[id] ?? id;

  return (
    <div className={styles['knowledge-ref-bar']}>
      {refs.notebookIds.map((id) => {
        const name = nameOf(id);
        return (
          <span key={id} className={styles['media-attachment-chip']} title={name}>
            <span className={styles['knowledge-ref-chip-icon']} aria-hidden="true">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
                <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
              </svg>
            </span>
            <span className={styles['media-attachment-name']}>{name}</span>
            <button
              type="button"
              className={styles['media-attachment-remove']}
              onClick={() => removeKnowledgeNotebook(sessionKey, id)}
              aria-label={t('input.knowledgeRemoveNotebook', { name })}
            >
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </span>
        );
      })}
      <span className={styles['knowledge-ref-mode']} role="group" aria-label={t('input.knowledgeModeLabel')}>
        {(['fast', 'detailed'] as KnowledgeReferenceMode[]).map((mode) => (
          <button
            key={mode}
            type="button"
            className={`${styles['knowledge-ref-mode-btn']}${refs.mode === mode ? ` ${styles.active}` : ''}`}
            title={t(mode === 'fast' ? 'input.knowledgeModeFastHint' : 'input.knowledgeModeDetailedHint')}
            aria-pressed={refs.mode === mode}
            onClick={() => setKnowledgeReferenceMode(sessionKey, mode)}
          >
            {t(mode === 'fast' ? 'input.knowledgeModeFast' : 'input.knowledgeModeDetailed')}
          </button>
        ))}
      </span>
    </div>
  );
});
