/** 思考与工具共用固定图标行，不随悬停或展开替换图标。 */
import { memo, useState } from 'react';
import type { DeferredHistoryContent } from '../../stores/chat-types';
import { useDeferredHistoryContent } from '../../hooks/use-deferred-history-content';
import { ActivityIcon, CopyActivity, activityText as t } from './MessageActivity';
import styles from './MessageActivity.module.css';

interface Props { content: string; sealed: boolean; sessionPath?: string; deferred?: DeferredHistoryContent; }
export const ThinkingBlock = memo(function ThinkingBlock({ content, sealed, sessionPath = '', deferred }: Props) {
  const [open, setOpen] = useState(false);
  const loaded = useDeferredHistoryContent(sessionPath, deferred, open && !!sessionPath);
  const displayed = loaded.data?.content ?? content;
  const lines = displayed.split('\n').map(line => line.trim()).filter(Boolean);
  const preview = (sealed ? lines[0] : lines.at(-1)) || t(sealed ? 'unavailable' : 'pending');
  return <div className={styles.activity} data-thinking="" data-status={sealed ? 'succeeded' : 'running'}>
    <button type="button" className={styles.row} aria-expanded={open} onClick={() => setOpen(value => !value)}><ActivityIcon kind="thinking" /><span className={styles.label}>{t('labels.thinking')}</span><span className={styles.separator} aria-hidden="true">·</span><span className={styles.summary}>{preview}</span></button>
    {open && <div className={styles.panel}><div className={styles.header}><span className={styles.title}>{t('labels.thinking')}</span><CopyActivity content={displayed} disabled={!displayed || loaded.loading || !!loaded.error || (!!deferred && !sessionPath)} /></div>{loaded.loading && <div className={styles.prose}>{t('loading')}</div>}{(loaded.error || (deferred && !sessionPath)) && <div className={styles.prose}>{t(loaded.error ? 'loadFailed' : 'unavailable')}</div>}<div className={styles.prose}>{displayed || preview}</div></div>}
  </div>;
});
