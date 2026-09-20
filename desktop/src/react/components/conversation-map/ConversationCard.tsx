import { memo, useMemo } from 'react';
import type { CSSProperties, MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from 'react';
import type { MapCard } from './conversation-map-layout';
import { MarkdownContent } from '../chat/MarkdownContent';
import { renderMarkdown } from '../../utils/markdown';
import { useI18n } from '../../hooks/use-i18n';
import styles from './ConversationMap.module.css';

interface Props {
  card: MapCard;
  threadTitle: string;
  isCurrent: boolean;
  isSelected: boolean;
  isCollapsed: boolean;
  isLastInThread: boolean;
  hiddenCount: number;
  hasDescendants: boolean;
  dragging: boolean;
  onSelect: (card: MapCard) => void;
  onToggleCollapse: (card: MapCard) => void;
  onOpenDraft: (card: MapCard) => void;
  onHeaderPointerDown: (card: MapCard, event: ReactPointerEvent<HTMLElement>) => void;
  onAnswerMouseUp: (card: MapCard, event: ReactMouseEvent<HTMLDivElement>) => void;
}

export const ConversationCard = memo(function ConversationCard({
  card,
  threadTitle,
  isCurrent,
  isSelected,
  isCollapsed,
  isLastInThread,
  hiddenCount,
  hasDescendants,
  dragging,
  onSelect,
  onToggleCollapse,
  onOpenDraft,
  onHeaderPointerDown,
  onAnswerMouseUp,
}: Props) {
  const { t } = useI18n();

  const answerHtml = useMemo(
    () => (card.answer ? renderMarkdown(card.answer) : ''),
    [card.answer],
  );

  let cls = styles.card;
  if (card.turnIndex === -1) cls += ` ${styles.cardPlaceholder}`;
  if (isCurrent) cls += ` ${styles.cardCurrent}`;
  if (isSelected) cls += ` ${styles.cardSelected}`;
  if (dragging) cls += ` ${styles.cardDragging}`;

  const style: CSSProperties = {
    left: card.x,
    top: card.y,
    ['--card-accent' as string]: card.color,
  };

  if (card.turnIndex === -1) {
    return (
      <div
        className={cls}
        style={style}
        data-map-card=""
        onClick={() => onSelect(card)}
      >
        <div className={styles.placeholderText}>{card.question}</div>
      </div>
    );
  }

  return (
    <div
      className={cls}
      style={style}
      data-map-card=""
      onClick={() => onSelect(card)}
    >
      <div
        className={styles.cardHeader}
        data-drag-card=""
        onPointerDown={(event) => onHeaderPointerDown(card, event)}
      >
        <span className={styles.cardDot} style={{ background: card.color }} />
        <span className={styles.cardTitle}>{threadTitle} · {card.turnIndex + 1}</span>
        {card.processCount > 0 && (
          <span className={styles.toolBadge}>
            {t('map.toolCount', { count: card.processCount })}
          </span>
        )}
        {(hasDescendants || isCollapsed) && (
          <button
            type="button"
            className={styles.collapseButton}
            title={isCollapsed ? t('map.expand') : t('map.collapse')}
            onClick={(event) => {
              event.stopPropagation();
              onToggleCollapse(card);
            }}
            onPointerDown={(event) => event.stopPropagation()}
          >
            {isCollapsed ? `▸ ${hiddenCount}` : '▾'}
          </button>
        )}
      </div>
      <div className={styles.cardBody}>
        {card.question && <div className={styles.question}>{card.question}</div>}
        {answerHtml && (
          <div
            className={styles.answer}
            data-map-answer=""
            onMouseUp={(event) => onAnswerMouseUp(card, event)}
          >
            <MarkdownContent html={answerHtml} />
          </div>
        )}
      </div>
      {isCollapsed && hiddenCount > 0 && (
        <div className={styles.cardFooter}>{t('map.hiddenCount', { count: hiddenCount })}</div>
      )}
      {!isCollapsed && card.truncated && (
        <div className={styles.cardFooter}>{t('map.truncated')}</div>
      )}
      <button
        type="button"
        className={styles.cardAddButton}
        title={isLastInThread ? t('map.continue') : t('map.branch')}
        onClick={(event) => {
          event.stopPropagation();
          onOpenDraft(card);
        }}
        onPointerDown={(event) => event.stopPropagation()}
      >
        +
      </button>
    </div>
  );
});
