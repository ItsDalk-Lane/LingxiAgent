import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { surfaceForElement, useWindowSurface } from '../../ui/window-surface';
import { useKnowledgeCitationResource } from './knowledge-citation-scope';
import styles from './KnowledgeCitation.module.css';

/** 引用悬浮预览只显示已保存的引文，不另行搜索或猜测原文。 */
export function KnowledgeCitationPreview({ citationId, anchor, id, onEnter, onLeave, onClose }: {
  citationId: string; anchor: HTMLAnchorElement; id: string;
  onEnter: () => void; onLeave: () => void; onClose: () => void;
}) {
  const { resolved, failed, retry } = useKnowledgeCitationResource(citationId);
  const panelRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);
  const fallbackSurface = useWindowSurface();
  const surface = useMemo(() => surfaceForElement(anchor, fallbackSurface), [anchor, fallbackSurface]);
  const t = window.t ?? ((key: string) => key);
  const sourceName = resolved?.source.displayName.split(/[\\/]/).pop() || t('knowledge.citationSource');
  const updatePosition = useCallback(() => {
    const panel = panelRef.current;
    if (!anchor.isConnected) { onClose(); return; }
    if (!panel) return;
    const target = anchor.getBoundingClientRect();
    const box = panel.getBoundingClientRect();
    const width = surface.document.documentElement.clientWidth || surface.window.innerWidth;
    const height = surface.document.documentElement.clientHeight || surface.window.innerHeight;
    const gap = 8;
    const preferredTop = target.top - box.height - gap;
    const top = preferredTop >= gap ? preferredTop : target.bottom + gap;
    const next = {
      top: Math.max(gap, Math.min(top, height - box.height - gap)),
      left: Math.max(gap, Math.min(target.left + (target.width - box.width) / 2, width - box.width - gap)),
    };
    setPosition(previous => previous?.top === next.top && previous.left === next.left ? previous : next);
  }, [anchor, onClose, surface]);
  useLayoutEffect(() => { updatePosition(); }, [updatePosition, resolved, failed]);
  useEffect(() => {
    surface.window.addEventListener('resize', updatePosition);
    surface.window.addEventListener('scroll', updatePosition, true);
    return () => {
      surface.window.removeEventListener('resize', updatePosition);
      surface.window.removeEventListener('scroll', updatePosition, true);
    };
  }, [surface, updatePosition]);
  useEffect(() => {
    anchor.setAttribute('aria-describedby', id);
    return () => { if (anchor.getAttribute('aria-describedby') === id) anchor.removeAttribute('aria-describedby'); };
  }, [anchor, id]);
  return createPortal(
    <div ref={panelRef} id={id} role="tooltip" tabIndex={0} className={styles.preview}
      style={{ top: position?.top ?? 0, left: position?.left ?? 0, visibility: position ? 'visible' : 'hidden' }}
      onMouseEnter={onEnter} onMouseLeave={onLeave} onFocus={onEnter} onBlur={onLeave}
      onKeyDown={event => { if (event.key === 'Escape') { event.stopPropagation(); onClose(); } }}>
      {resolved ? <>
        <div className={styles.header}>
          <svg className={styles.fileIcon} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" aria-hidden="true">
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <path d="M14 2v6h6M8 13h8M8 17h6" />
          </svg>
          <span className={styles.title} title={sourceName}>{sourceName}</span>
        </div>
        <p className={styles.excerpt}>{resolved.citation.canonicalText}</p>
      </> : failed ? <div role="alert" className={styles.status}>
        <p>{t('knowledge.citationFailed')}</p>
        <button type="button" className={styles.retry} onClick={retry}>{t('knowledge.citationRetry')}</button>
      </div> : <p className={styles.status} aria-live="polite">{t('knowledge.citationLoading')}</p>}
    </div>, surface.overlayRoot,
  );
}
