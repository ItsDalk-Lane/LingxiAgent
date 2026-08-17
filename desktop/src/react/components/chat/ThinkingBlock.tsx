/**
 * ThinkingBlock — 可折叠的思考过程区块
 */

import { memo, useState, useCallback } from 'react';
import { Collapse } from '@/ui';
import type { DeferredHistoryContent } from '../../stores/chat-types';
import { useDeferredHistoryContent } from '../../hooks/use-deferred-history-content';
import styles from './Chat.module.css';

interface Props {
  content: string;
  sealed: boolean;
  sessionPath?: string;
  deferred?: DeferredHistoryContent;
}

export const ThinkingBlock = memo(function ThinkingBlock({ content, sealed, sessionPath = '', deferred }: Props) {
  const t = window.t ?? ((p: string) => p);
  const [open, setOpen] = useState(false);
  const toggle = useCallback(() => setOpen(v => !v), []);
  const loaded = useDeferredHistoryContent(sessionPath, deferred, open && !!sessionPath);
  const displayedContent = loaded.data?.content || content;

  return (
    <details className={styles.thinkingBlock} open={open} onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}>
      <summary className={styles.thinkingBlockSummary} onClick={(e) => { e.preventDefault(); toggle(); }}>
        <span className={`${styles.thinkingBlockArrow}${open ? ` ${styles.thinkingBlockArrowOpen}` : ''}`}>›</span>
        {' '}{sealed ? t('thinking.done') : (
          <>{t('thinking.active')}<span className={styles.thinkingDots} /></>
        )}
      </summary>
      <Collapse open={open && !!displayedContent}>
        <div className={styles.thinkingBlockBody}>{displayedContent}</div>
      </Collapse>
    </details>
  );
});
