/**
 * ConversationMapInspector — 卡片详情面板（地图页右侧浮层）
 *
 * 展示完整问答与动作行：在对话中打开 / 创建分支 / 继续追问。
 */

import { useMemo } from 'react';
import { useStore } from '../../stores';
import { switchSession } from '../../stores/session-actions';
import { MarkdownContent } from '../chat/MarkdownContent';
import { renderMarkdown } from '../../utils/markdown';
import { useI18n } from '../../hooks/use-i18n';
import type { MapCard } from './conversation-map-layout';
import styles from './ConversationMap.module.css';

interface Props {
  card: MapCard;
  threadTitle: string;
  /** 该卡片是否为其 thread 的最后一张可见卡片（决定能否继续追问）。 */
  isLastInThread: boolean;
  onClose: () => void;
}

export function ConversationMapInspector({ card, threadTitle, isLastInThread, onClose }: Props) {
  const { t } = useI18n();
  const setMapDraft = useStore((s) => s.setMapDraft);

  const answerHtml = useMemo(
    () => (card.answer ? renderMarkdown(card.answer) : ''),
    [card.answer],
  );

  const openDraft = (kind: 'continue' | 'branch') => {
    setMapDraft({
      kind,
      anchorCardId: card.id,
      sessionId: card.sessionId,
      sessionPath: card.sessionPath,
      agentId: card.agentId,
      text: '',
      sending: false,
    });
  };

  const canBranch = Boolean(card.answerEntryId || card.questionEntryId);

  return (
    <div className={styles.inspector} data-map-inspector="">
      <div className={styles.inspectorHeader}>
        <span className={styles.inspectorTitle}>
          {threadTitle} · {t('map.turnLabel', { index: card.turnIndex + 1 })}
        </span>
        <button
          type="button"
          className={styles.inspectorClose}
          aria-label={t('map.cancel')}
          onClick={onClose}
        >
          ✕
        </button>
      </div>
      <div className={styles.inspectorBody}>
        {card.question && (
          <div className={styles.inspectorQuestion}>{card.question}</div>
        )}
        {answerHtml && (
          <div className={styles.inspectorAnswer}>
            <MarkdownContent html={answerHtml} />
          </div>
        )}
        {card.processCount > 0 && (
          <div className={styles.inspectorMeta}>
            {t('map.processLine', { count: card.processCount })}
          </div>
        )}
        {card.error && (
          <div className={styles.inspectorError}>{card.error}</div>
        )}
      </div>
      <div className={styles.inspectorActions}>
        <button
          type="button"
          className={styles.toolbarButton}
          onClick={() => {
            void switchSession(card.sessionPath);
            useStore.getState().setCurrentTab('chat');
          }}
        >
          {t('map.openInChat')}
        </button>
        {canBranch && (
          <button
            type="button"
            className={styles.toolbarButton}
            onClick={() => openDraft('branch')}
          >
            {t('map.branch')}
          </button>
        )}
        {isLastInThread && (
          <button
            type="button"
            className={styles.toolbarButton}
            onClick={() => openDraft('continue')}
          >
            {t('map.continue')}
          </button>
        )}
      </div>
    </div>
  );
}
