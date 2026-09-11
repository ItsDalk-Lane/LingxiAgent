/**
 * TodoPanel — 主聊天输入框上方的清单进度条（清单改版阶段 3）
 *
 * 展示规则（PLAN.md §4）：
 * - 与输入框内容宽度对齐，随输入区固定显示；没有清单时完全不占位置。
 * - 默认收起；标题整行可点击（button 原生支持 Enter/空格），左侧固定清单图标。
 * - 标题按状态显示非零数量；收起时显示首个进行中步骤，多项并行补充
 *   "另有 n 项进行中"，不隐藏并行事实。
 * - 展开后显示完整清单（五态图标、受阻原因、已取消标注），内部滚动
 *   最高约 240px 且不超过窗口高度 30%；长描述可换行。
 * - 任务完成状态由模型确定，面板不提供手动完成/取消入口；
 *   唯一的收尾操作是收纳已结束清单摘要（只隐藏，不改结果）。
 * - 本轮已停止且仍有未完成任务时，进行中图标从旋转动效变为静止，
 *   并提示真实含义（A06）。
 * - 展开偏好按会话在本次运行期间保存（store: todoPanelExpandedBySession），
 *   流式更新不重置展开状态（A03）。
 */
import { memo, useState } from 'react';
import { useStore } from '../../stores';
import { dismissSessionTodoPanel } from '../../stores/session-actions';
import { sessionScopedListIncludes, sessionScopedValue } from '../../stores/session-slice';
import type { TodoItem } from '../../types';
import styles from './TodoPanel.module.css';

const EMPTY_PANEL_TODOS: TodoItem[] = [];

function ListIcon() {
  return (
    <svg className={styles.titleIcon} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <line x1="9" y1="6" x2="21" y2="6" /><line x1="9" y1="12" x2="21" y2="12" /><line x1="9" y1="18" x2="21" y2="18" />
      <circle cx="4.5" cy="6" r="1.3" /><circle cx="4.5" cy="12" r="1.3" /><circle cx="4.5" cy="18" r="1.3" />
    </svg>
  );
}

function StatusIcon({ status, stopped }: { status: TodoItem['status']; stopped: boolean }) {
  if (status === 'in_progress') {
    // 本轮已停止：进行中图标不再表现为仍在执行（静态半圆，无旋转动效）。
    return (
      <svg className={`${styles.statusIcon} ${stopped ? styles.stoppedIcon : styles.activeIcon}`} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
        {stopped ? <path d="M12 3a9 9 0 1 0 9 9" /> : <path d="M21 3v5m0 0h-5m5 0-3-2.708A9 9 0 1 0 20.777 14" strokeLinejoin="round" />}
      </svg>
    );
  }
  if (status === 'completed') {
    return (
      <svg className={`${styles.statusIcon} ${styles.completedIcon}`} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="9" /><path d="M8.5 12.5l2.5 2.5 4.5-5" />
      </svg>
    );
  }
  if (status === 'blocked') {
    return (
      <svg className={`${styles.statusIcon} ${styles.blockedIcon}`} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M12 3 2.5 20h19L12 3Z" /><line x1="12" y1="10" x2="12" y2="14" /><circle cx="12" cy="17" r="0.4" fill="currentColor" />
      </svg>
    );
  }
  if (status === 'cancelled') {
    return (
      <svg className={`${styles.statusIcon} ${styles.cancelledIcon}`} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="9" /><line x1="8" y1="8" x2="16" y2="16" />
      </svg>
    );
  }
  return (
    <svg className={`${styles.statusIcon} ${styles.pendingIcon}`} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
      <circle cx="12" cy="12" r="8.2" />
    </svg>
  );
}

function displayText(todo: TodoItem): string {
  if (todo.status === 'in_progress' && todo.activeForm) return todo.activeForm;
  return todo.content || todo.activeForm || '';
}

type Counts = Record<TodoItem['status'], number>;

function countByStatus(todos: TodoItem[]): Counts {
  const counts: Counts = { pending: 0, in_progress: 0, blocked: 0, cancelled: 0, completed: 0 };
  for (const td of todos) counts[td.status] = (counts[td.status] || 0) + 1;
  return counts;
}

export const TodoPanel = memo(function TodoPanel() {
  const [acting, setActing] = useState(false);
  const sessionPath = useStore((s) => s.currentSessionPath);
  const panel = useStore((s) => {
    const path = s.currentSessionPath;
    return path ? sessionScopedValue(s, s.todoPanelBySession, path) ?? null : null;
  });
  const streaming = useStore((s) =>
    sessionScopedListIncludes(s, s.streamingSessions, s.currentSessionPath),
  );
  const expanded = useStore((s) => {
    const path = s.currentSessionPath;
    return path ? sessionScopedValue(s, s.todoPanelExpandedBySession, path) === true : false;
  });
  const t = window.t ?? ((k: string) => k);

  if (!sessionPath || !panel) return null;
  const todos = panel.todos ?? EMPTY_PANEL_TODOS;
  // 没有条目且没有失败标记时不占位（防御：正常路径不会写入这种快照）。
  if (todos.length === 0 && !panel.updateFailed) return null;

  const counts = countByStatus(todos);
  const unfinished = counts.pending + counts.in_progress + counts.blocked;
  const finished = panel.finished && todos.length > 0;
  const stopped = !streaming && !finished && unfinished > 0;

  const parts: string[] = [];
  if (counts.completed > 0) parts.push(t('todoPanel.completed', { n: counts.completed }));
  if (counts.in_progress > 0) parts.push(t('todoPanel.inProgress', { n: counts.in_progress }));
  if (counts.pending > 0) parts.push(t('todoPanel.pending', { n: counts.pending }));
  if (counts.blocked > 0) parts.push(t('todoPanel.blocked', { n: counts.blocked }));
  if (counts.cancelled > 0) parts.push(t('todoPanel.cancelled', { n: counts.cancelled }));

  let title: string;
  if (finished && panel.allCompleted) {
    title = t('todoPanel.allDone', { n: todos.length });
  } else if (finished) {
    title = `${t('todoPanel.finishedPrefix')} · ${parts.join(' · ')}`;
  } else {
    title = parts.join(' · ');
  }

  const inProgressItems = todos.filter((td) => td.status === 'in_progress');
  const firstInProgress = inProgressItems[0];

  async function runAction(action: (path: string) => Promise<boolean>) {
    if (acting || !sessionPath) return;
    setActing(true);
    try {
      await action(sessionPath);
    } finally {
      setActing(false);
    }
  }

  function toggleExpanded() {
    if (!sessionPath) return;
    useStore.getState().setSessionTodoPanelExpanded(sessionPath, !expanded);
  }

  return (
    <section className={styles.panel} aria-label={t('todoPanel.title')} data-todo-panel="">
      <button
        type="button"
        className={styles.header}
        aria-expanded={expanded}
        aria-label={`${t('todoPanel.title')} · ${title}`}
        onClick={toggleExpanded}
      >
        <ListIcon />
        <span className={styles.title}>{t('todoPanel.title')}</span>
        {title && <span className={styles.counts}>{title}</span>}
        {!expanded && firstInProgress && (
          <span className={styles.current}>
            {displayText(firstInProgress)}
            {inProgressItems.length > 1 && (
              <span className={styles.moreParallel}>
                {t('todoPanel.moreInProgress', { n: inProgressItems.length - 1 })}
              </span>
            )}
          </span>
        )}
      </button>
      {panel.updateFailed && (
        <div className={styles.notice} role="status">{t('todoPanel.updateFailed')}</div>
      )}
      {stopped && (
        <div className={styles.stoppedNote} role="status">{t('todoPanel.stopped')}</div>
      )}
      {expanded && (
        <>
          <div className={styles.list} role="list">
            {todos.map((td, i) => (
              <div key={`todo-${i}`} className={styles.row} data-status={td.status} role="listitem">
                <StatusIcon status={td.status} stopped={stopped} />
                <div className={styles.rowBody}>
                  <span className={styles.text}>
                    {displayText(td)}
                    {td.status === 'cancelled' && (
                      <span className={styles.cancelledTag}>{t('todoPanel.cancelledLabel')}</span>
                    )}
                  </span>
                  {td.status === 'blocked' && td.blockedReason && (
                    <span className={styles.reason}>
                      {t('todoPanel.blockedReason', { reason: td.blockedReason })}
                    </span>
                  )}
                </div>
              </div>
            ))}
          </div>
          <div className={styles.actions}>
            {finished && (
              <button
                type="button"
                className={styles.action}
                disabled={acting}
                onClick={() => void runAction(dismissSessionTodoPanel)}
              >
                {t('todoPanel.dismiss')}
              </button>
            )}
          </div>
        </>
      )}
    </section>
  );
});
