import { memo, useEffect, useState } from 'react';
import { ChatResourceCard } from './ChatResourceCard';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { useStore } from '../../stores';
import styles from './Chat.module.css';

/**
 * AutolearnLessonCard — 踩坑自动沉淀的建议卡（kind: autolearn_lesson）
 *
 * server 侧 autolearn-service 在 turn 结束后提炼可复用教训，经 ConfirmStore
 * 发出这张卡；用户点「学会它」才经 learn_lesson 落盘核心写进技能池，
 * 「忽略」或超时（24h）都直接作废——卡片本身不落盘，刷新/重开会话后
 * 不再出现；ConfirmStore 条目过期后确认接口返回 404，此时如实刻画「已过期」。
 */
export const AutolearnLessonCard = memo(function AutolearnLessonCard({ block }: { block: any; sessionPath?: string }) {
  const detail = block.detail || {};
  const name = (detail.name as string) || (block.title as string) || '';
  const description = (detail.description as string) || (block.description as string) || '';
  const lesson = (detail.lesson as string) || '';

  const addToast = useStore(s => s.addToast);
  const [status, setStatus] = useState(block.status);
  const [expired, setExpired] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  // pending 默认展开：决策按钮住在展开区，不能开局藏起来；已了结的卡默认收起。
  const [expanded, setExpanded] = useState(block.status === 'pending');

  useEffect(() => {
    setStatus(block.status);
  }, [block.status]);

  // 超时有两种到达方式：ws confirmation_resolved 把 block.status 改成 'timeout'，
  // 或用户点按钮时 ConfirmStore 条目已过期（404）。两者都如实刻画「已过期」。
  const timedOut = expired || status === 'timeout';
  const pending = status === 'pending' && !timedOut;

  const decide = async (action: 'confirmed' | 'rejected') => {
    if (submitting || !block.confirmId) return;
    setSubmitting(true);
    try {
      const res = await lingxiFetch(`/api/confirm/${block.confirmId}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ action }),
        throwOnHttpError: false,
      });
      if (res.status === 404) {
        // ConfirmStore 条目已过期：建议作废，如实刻画，不装作操作成功。
        setExpired(true);
        return;
      }
      if (!res.ok) {
        const data = await res.json().catch(() => ({}));
        throw new Error((data?.error as string) || res.statusText);
      }
      setStatus(action === 'confirmed' ? 'approved' : 'rejected');
    } catch (err: any) {
      addToast(window.t('autolearn.decideFailed', { error: err?.message || String(err) }), 'error');
    } finally {
      setSubmitting(false);
    }
  };

  const statusLabel = timedOut
    ? window.t('autolearn.expiredState')
    : status === 'approved'
      ? window.t('autolearn.approvedState')
      : status === 'rejected'
        ? window.t('autolearn.rejectedState')
        : undefined;
  const statusTone = timedOut || status === 'rejected' ? 'muted' : status === 'approved' ? 'success' : 'accent';
  const hasBody = pending || !!lesson;

  return (
    <ChatResourceCard
      icon={<span className={styles.autolearnLessonIcon} aria-hidden="true">💡</span>}
      title={name || window.t('autolearn.cardTitle')}
      titleMeta={pending ? window.t('autolearn.suggested') : undefined}
      subtitle={description}
      statusLabel={statusLabel}
      statusTone={statusTone}
      className={styles.autolearnLessonCard}
      expandable={hasBody}
      expanded={expanded}
      onToggle={hasBody ? () => setExpanded(v => !v) : undefined}
      ariaLabel={name || window.t('autolearn.cardTitle')}
    >
      {hasBody ? (
        <div className={styles.autolearnLessonBody}>
          {lesson && <div className={styles.autolearnLessonText}>{lesson}</div>}
          {pending && (
            <div className={styles.automationDraftActions}>
              <button
                className={styles.automationDraftTextButton}
                type="button"
                onClick={() => void decide('rejected')}
                disabled={submitting}
              >
                {window.t('autolearn.dismiss')}
              </button>
              <button
                className={styles.automationDraftPrimaryButton}
                type="button"
                onClick={() => void decide('confirmed')}
                disabled={submitting}
              >
                {window.t('autolearn.approve')}
              </button>
            </div>
          )}
        </div>
      ) : null}
    </ChatResourceCard>
  );
});
