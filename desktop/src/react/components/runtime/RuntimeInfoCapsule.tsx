/**
 * RuntimeInfoCapsule — 运行信息悬浮胶囊（桌面 chat 页右上角）
 *
 * 收起：小胶囊（状态图标 + 运行中条目数：running terminal / workflow / subagent，
 *       数据源复用现有 store selector，不新建数据链路）。
 * 展开：胶囊自身长成统一圆角容器（单一界面，不另起弹出面板），胶囊头保留为
 *       容器标题行，下方按 Section 收纳既有运行期内容：
 *       笺（JianEditor）、Session Todo、Terminal、Workflow、Agent Activity、Session Status。
 *       卡片的卡片皮肤在容器内被压平（背景/边框/阴影置透明 + 分隔线），形成单一容器。
 *
 * 浮层不占文档流，展开/收起不改变 Chat Transcript 宽度。
 * 无 session 时各运行卡自身 return null，笺始终可用，容器不会空。
 */

import { useEffect, useRef, useState } from 'react';
import { useStore } from '../../stores';
import { selectTerminals } from '../../stores/terminal-slice';
import { selectAgentActivities } from '../../stores/agent-activity-slice';
import { JianEditor } from '../desk/DeskEditor';
import { SessionTodoCard } from '../right-workspace/SessionTodoCard';
import { TerminalCard } from '../right-workspace/TerminalCard';
import { WorkflowCard } from '../right-workspace/WorkflowCard';
import { AgentActivityCard } from '../right-workspace/AgentActivityCard';
import { SessionStatusCard } from '../right-workspace/SessionStatusCard';
import styles from './RuntimeInfoCapsule.module.css';

export function RuntimeInfoCapsule() {
  const [expanded, setExpanded] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const sessionPath = useStore(s => s.currentSessionPath);
  const terminals = useStore(selectTerminals(sessionPath));
  const activities = useStore(selectAgentActivities(sessionPath));
  const t = window.t ?? ((p: string) => p);

  const runningCount =
    terminals.filter(terminal => terminal.status === 'running').length
    + activities.filter(entry =>
      (entry.kind === 'workflow' || entry.kind === 'subagent') && entry.status === 'running',
    ).length;

  // 展开时点击胶囊外任意处收起（捕获阶段，兼容面板内 stopPropagation 的控件）
  useEffect(() => {
    if (!expanded) return;
    const onDocumentMouseDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setExpanded(false);
      }
    };
    document.addEventListener('mousedown', onDocumentMouseDown, true);
    return () => document.removeEventListener('mousedown', onDocumentMouseDown, true);
  }, [expanded]);

  const actionLabel = expanded ? t('runtimeCapsule.collapse') : t('runtimeCapsule.expand');

  return (
    <div
      className={`${styles.root}${expanded ? ` ${styles.expanded}` : ''}`}
      ref={rootRef}
      data-runtime-capsule=""
      data-expanded={expanded ? 'true' : 'false'}
    >
      <button
        type="button"
        className={styles.pill}
        aria-expanded={expanded}
        aria-label={actionLabel}
        title={actionLabel}
        onClick={() => setExpanded(v => !v)}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <polyline points="22 12 18 12 15 21 9 3 6 12 2 12"></polyline>
        </svg>
        <span className={styles.pillLabel}>
          {runningCount > 0 ? t('runtimeCapsule.running', { n: runningCount }) : t('runtimeCapsule.title')}
        </span>
      </button>
      {expanded && (
        <div className={styles.panelBody} role="dialog" aria-label={t('runtimeCapsule.title')}>
          <section className={styles.jianSection} aria-label={t('desk.jianLabel')}>
            <div className={styles.jianHeader}>
              <span className={styles.jianTitle}>{t('desk.jianLabel')}</span>
            </div>
            <div className={styles.jianBody}>
              <JianEditor showHeader={false} />
            </div>
          </section>
          <SessionTodoCard />
          <TerminalCard />
          <WorkflowCard />
          <AgentActivityCard />
          <SessionStatusCard />
        </div>
      )}
    </div>
  );
}
