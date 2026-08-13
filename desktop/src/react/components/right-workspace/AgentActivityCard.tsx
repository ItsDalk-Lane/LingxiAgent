import { useEffect, useRef, useState } from 'react';
import { useStore } from '../../stores';
import { selectAgentActivities, type AgentActivityEntry } from '../../stores/agent-activity-slice';
import { AgentAvatar, resolveAgentDisplayInfo } from '../../utils/agent-display';
import { formatElapsed } from '../../utils/format-duration';
import { stopSubagentProcess } from '../../services/background-process-control';
import { navigateToChatCard } from '../../services/chat-card-navigation';
import type { Agent } from '../../types';
import styles from './AgentActivityCard.module.css';

// stopping 兜底复位：3 倍于 background-process-control 的 REQUEST_TIMEOUT_MS（10s）。
const STOPPING_FALLBACK_RESET_MS = 30_000;

function SubagentActivityRow({
  entry,
  agents,
  now,
  sessionId,
  sessionPath,
}: {
  entry: AgentActivityEntry;
  agents: Agent[];
  now: number;
  sessionId: string | null;
  sessionPath: string;
}) {
  const [stopping, setStopping] = useState(false);
  const stoppingFallbackTimerRef = useRef<number | null>(null);
  const t = window.t ?? ((key: string) => key);
  const info = resolveAgentDisplayInfo({
    id: entry.agentId,
    agents,
    fallbackAgentName: entry.agentName || entry.agentId || 'Subagent',
  });
  const title = entry.summary || entry.label || info.displayName;

  useEffect(() => () => {
    if (stoppingFallbackTimerRef.current !== null) window.clearTimeout(stoppingFallbackTimerRef.current);
  }, []);

  const handleStop = async () => {
    if (stopping) return;
    setStopping(true);
    try {
      await stopSubagentProcess({ sessionId, sessionPath, taskId: entry.id });
      // 正常路径由权威 agent_activity 事件把这行移除；事件丢失时用超时兜底复位，
      // 避免按钮永久 disabled。
      stoppingFallbackTimerRef.current = window.setTimeout(
        () => setStopping(false),
        STOPPING_FALLBACK_RESET_MS,
      );
    } catch {
      useStore.getState().addToast(t('rightWorkspace.process.stopFailed'), 'error');
      setStopping(false);
    }
  };

  return (
    <div className={styles.item} data-status={entry.status}>
      <button
        type="button"
        className={styles.titleButton}
        data-subagent-title={entry.id}
        onClick={() => navigateToChatCard({ kind: 'subagent', ids: [entry.id], sessionPath })}
        title={title}
      >
        <span className={styles.avatar}>
          <AgentAvatar info={info} className={styles.avatarImg} alt={info.displayName} />
        </span>
        <span className={styles.titleText}>{title}</span>
        {title !== info.displayName ? <span className={styles.agentName}>{info.displayName}</span> : null}
      </button>
      <div className={styles.footer}>
        <span>{t('rightWorkspace.subagent.runningFor', { text: formatElapsed(now - (entry.startedAt || now)) })}</span>
        <button
          type="button"
          className={styles.stopButton}
          disabled={stopping}
          onClick={() => void handleStop()}
        >
          <span aria-hidden="true">■</span>
          {stopping ? t('rightWorkspace.process.stopping') : t('rightWorkspace.subagent.stop')}
        </button>
      </div>
    </div>
  );
}

export function AgentActivityCard() {
  const sessionId = useStore((state) => state.currentSessionId);
  const sessionPath = useStore((state) => state.currentSessionPath);
  const all = useStore(selectAgentActivities(sessionPath));
  const agents = useStore((state) => state.agents);
  const [now, setNow] = useState(() => Date.now());
  const t = window.t ?? ((key: string) => key);
  const activities = all
    .filter((entry) => entry.kind === 'subagent' && entry.status === 'running')
    .sort((a, b) => (b.startedAt ?? 0) - (a.startedAt ?? 0));

  useEffect(() => {
    if (!activities.length) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [activities.length]);

  if (!sessionPath || !activities.length) return null;

  return (
    <section className={`universal-card ${styles.card}`} aria-label={t('rightWorkspace.subagent.title')}>
      <div className={styles.header}>
        <span className={styles.title}>{t('rightWorkspace.subagent.title')}</span>
        <span className={styles.count}>{t('rightWorkspace.subagent.count', { n: activities.length })}</span>
      </div>
      <div className={styles.list}>
        {activities.map((entry) => (
          <SubagentActivityRow
            key={entry.id}
            entry={entry}
            agents={agents}
            now={now}
            sessionId={sessionId}
            sessionPath={sessionPath}
          />
        ))}
      </div>
    </section>
  );
}
