/**
 * HistoryOverviewBadge — E05 概览最小展示。
 *
 * 挂载在既有「加载更早历史」提示区（loadMoreHint）内，不新增顶级入口/统计面板，
 * 不改变整体布局。非阻塞：effect 中按需请求一次（在途合并），首屏渲染不依赖。
 * 文案明确区分「原始历史记录」「逻辑轮次」「关联任务」，Run 数不表述为完成任务数。
 */
import { memo, useSyncExternalStore, useEffect, useRef } from 'react';
import {
  requestHistoryOverview,
  subscribeHistoryOverview,
  getHistoryOverviewSnapshot,
} from '../../stores/history-overview-client';
import { useI18n } from '../../hooks/use-i18n';
import styles from './Chat.module.css';

interface Props {
  sessionPath: string;
  active: boolean;
  /** 页面已有内容（首个消息请求成功且已渲染）→ 才允许触发概览请求。 */
  hasRenderedContent: boolean;
}

export const HistoryOverviewBadge = memo(function HistoryOverviewBadge({
  sessionPath,
  active,
  hasRenderedContent,
}: Props) {
  const { t } = useI18n();
  const snapshot = useSyncExternalStore(
    (notify) => subscribeHistoryOverview(notify),
    () => getHistoryOverviewSnapshot(sessionPath),
    () => getHistoryOverviewSnapshot(sessionPath),
  );
  const requestedForRef = useRef('');

  // 非阻塞触发：页面已显示内容后按需请求一次；在途合并；失败静默。
  useEffect(() => {
    if (!active || !hasRenderedContent) return;
    if (snapshot.status !== 'idle') return;
    const key = `${sessionPath}`;
    if (requestedForRef.current === key) return;
    requestedForRef.current = key;
    void requestHistoryOverview(sessionPath);
  }, [active, hasRenderedContent, sessionPath, snapshot.status]);

  if (snapshot.status !== 'available' || !snapshot.data) {
    // unavailable/unsupported → 隐藏统计（不当成已加载到底，也不弹持续错误）
    return null;
  }
  const { data } = snapshot;
  const dist = data.taskDistribution;
  const detail = `${t('chat.historyOverview.taskSubagent')} ${dist.subagent} · ${t('chat.historyOverview.taskWorkflow')} ${dist.workflow} · ${t('chat.historyOverview.taskMedia')} ${dist.media} · ${t('chat.historyOverview.taskOther')} ${dist.other}`;
  return (
    <span
      className={styles.historyOverviewBadge}
      title={detail}
      data-history-overview=""
    >
      {t('chat.historyOverview.records')} {data.displayRecords}
      {' · '}
      {t('chat.historyOverview.runs')} {data.runsWithAssistant}
      {' · '}
      {t('chat.historyOverview.tasks')} {data.referencedTasks}
    </span>
  );
});
