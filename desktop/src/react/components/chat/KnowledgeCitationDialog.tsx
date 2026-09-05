import { useRef } from 'react';
import { Overlay } from '../../ui';
import { useKnowledgeCitationResource } from './knowledge-citation-scope';
import styles from '../knowledge/KnowledgePage.module.css';

/** 旧回答与新回答都按同一引用身份读取原文，不打开服务器文件路径。 */
export function KnowledgeCitationDialog({ citationId, onClose }: { citationId: string; onClose: () => void }) {
  const { resolved, failed } = useKnowledgeCitationResource(citationId);
  const closeRef = useRef<HTMLButtonElement>(null);
  const t = window.t ?? ((key: string) => key);
  const sourceName = resolved?.source.displayName.split(/[\\/]/).pop() || t('knowledge.citationSource');
  const locator = resolved?.viewer?.locator ?? resolved?.block.locator;
  const headings = Array.isArray(locator?.headingPath)
    ? locator.headingPath.filter((part): part is string => typeof part === 'string').join(' / ') : '';
  const pageNumber = locator?.pageNumber ?? locator?.page;
  const location = headings || (typeof pageNumber === 'number'
    ? t('knowledge.pageNumber', { number: pageNumber })
    : typeof locator?.lineStart === 'number' ? t('knowledge.lineNumber', { number: locator.lineStart })
      : resolved ? t('knowledge.paragraphNumber', { number: resolved.block.ordinal + 1 }) : '');
  return (
    <Overlay open scope="window" onClose={onClose} className={styles.chunkDialog}
      initialFocusRef={closeRef} contentProps={{ role: 'dialog', 'aria-modal': true, 'aria-label': t('knowledge.citationTitle') }}>
      <header className={styles.chunkDialogHeader}>
        <h2 className={styles.chunkDialogTitle}>{t('knowledge.citationTitle')}</h2>
        <button ref={closeRef} className={styles.iconButton} onClick={onClose} aria-label={t('common.close')}>×</button>
      </header>
      {resolved && <div className={styles.chunkDialogMeta}><span>{sourceName}</span><span>{location}</span></div>}
      <div className={styles.chunkDialogBody} aria-busy={!resolved && !failed}>
        {failed ? <p role="alert">{t('knowledge.citationFailed')}</p>
          : resolved ? <pre className={styles.chunkDialogText}>
            {resolved.block.text.slice(0, resolved.citation.startOffset)}
            <mark>{resolved.citation.canonicalText}</mark>
            {resolved.block.text.slice(resolved.citation.endOffset)}
          </pre> : <p>{t('knowledge.citationLoading')}</p>}
      </div>
    </Overlay>
  );
}
