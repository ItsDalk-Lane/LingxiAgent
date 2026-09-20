/**
 * ConversationMapPendingCard — 已发送、等待落盘的待回复卡片
 *
 * 草稿发出后立即挂出：问题先行展示，答复区轮询 streamBuffer 贴入流式文本；
 * 轮次落盘后由 ConversationMapPage 的 reconciliation 清除 pending 条目，
 * 无缝交接给正式卡片。待回复卡片不提供拖拽/折叠/选中/「+」等交互。
 */

import { useEffect, useMemo, useState } from 'react';
import type { CSSProperties } from 'react';
import { useStore } from '../../stores';
import { sessionScopedListIncludes } from '../../stores/session-slice';
import { snapshotStreamBuffer } from '../../stores/stream-invalidator';
import { useI18n } from '../../hooks/use-i18n';
import { MarkdownContent } from '../chat/MarkdownContent';
import { renderMarkdown } from '../../utils/markdown';
import { CARD_HEIGHT, CARD_WIDTH } from './conversation-map-layout';
import styles from './ConversationMap.module.css';

const STREAM_POLL_INTERVAL_MS = 150;

export function PendingMapAnswer({ sessionPath }: { sessionPath: string }) {
  const { t } = useI18n();
  const streaming = useStore((s) => sessionScopedListIncludes(s, s.streamingSessions, sessionPath));
  const [text, setText] = useState('');

  useEffect(() => {
    if (!streaming) {
      setText('');
      return;
    }
    const poll = () => {
      const snapshot = snapshotStreamBuffer(sessionPath);
      setText(snapshot?.text ?? '');
    };
    poll();
    const timer = setInterval(poll, STREAM_POLL_INTERVAL_MS);
    return () => clearInterval(timer);
  }, [sessionPath, streaming]);

  const answerHtml = useMemo(() => (text ? renderMarkdown(text) : ''), [text]);

  if (!answerHtml) {
    return <div className={styles.pendingHint}>{t('map.waitingForAssistant')}</div>;
  }
  return (
    <div className={styles.answer} data-map-answer="">
      <MarkdownContent html={answerHtml} />
    </div>
  );
}

interface Props {
  sessionPath: string;
  question: string;
  color: string;
  position: { x: number; y: number };
}

export function ConversationMapPendingCard({ sessionPath, question, color, position }: Props) {
  const { t } = useI18n();

  const style: CSSProperties = {
    left: position.x,
    top: position.y,
    width: CARD_WIDTH,
    minHeight: CARD_HEIGHT,
    ['--card-accent' as string]: color,
  };

  return (
    <div
      className={`${styles.draftCard} ${styles.pendingCard}`}
      style={style}
      data-map-card=""
      data-map-pending=""
    >
      <div className={styles.cardHeader}>
        <span className={styles.cardDot} style={{ background: color }} />
        <span className={styles.cardTitle}>{t('map.sending')}</span>
      </div>
      <div className={styles.draftBody}>
        {question && <div className={styles.question}>{question}</div>}
        <PendingMapAnswer sessionPath={sessionPath} />
      </div>
    </div>
  );
}
