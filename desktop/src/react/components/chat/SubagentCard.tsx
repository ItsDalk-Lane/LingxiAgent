/**
 * SubagentCard — 子 Agent 可展开详情卡片
 *
 * 收起时只显示静态任务与终态，展开后才挂载 child session 详情流。
 */

import { memo, useState, useEffect, useCallback, useRef } from 'react';
import { useStore } from '../../stores';
import { AgentAvatar, resolveAgentDisplayInfo } from '../../utils/agent-display';
import { stopSubagentProcess } from '../../services/background-process-control';
import { subscribeChatCardNavigation } from '../../services/chat-card-navigation';
import { ChatResourceCard } from './ChatResourceCard';
import { SubagentSessionPreview } from './SubagentSessionPreview';
import type { ChatResourceCardStatusTone } from './ChatResourceCard';
import styles from './Chat.module.css';

interface SubagentCardProps {
  block: {
    taskId: string;
    task: string;
    taskTitle: string;
    agentId?: string;
    agentName?: string;
    requestedAgentId?: string;
    requestedAgentName?: string;
    executorAgentId?: string;
    executorAgentNameSnapshot?: string;
    sessionId?: string | null;
    streamKey: string;
    streamStatus: 'running' | 'done' | 'failed' | 'aborted';
    summary?: string;
    label?: string | null;
    reuseInstance?: string | null;
  };
}

export const SubagentCard = memo(function SubagentCard({ block }: SubagentCardProps) {
  const [status, setStatus] = useState(block.streamStatus);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const t = window.t ?? ((key: string) => key);

  // 头像：优先用 agent 头像 API，fallback 到 yuan 剪影头像
  const currentAgentId = useStore(s => s.currentAgentId);
  const currentSessionId = useStore(s => s.currentSessionId);
  const currentSessionPath = useStore(s => s.currentSessionPath);
  const previewEntry = useStore(s => s.subagentPreviewByTaskId[block.taskId]);
  const agents = useStore(s => s.agents);
  const agentId = block.agentId || block.executorAgentId || currentAgentId || '';
  const displayInfo = resolveAgentDisplayInfo({
    id: agentId || null,
    agents,
    fallbackAgentName: block.agentName || block.executorAgentNameSnapshot || block.agentId || 'Subagent',
  });
  const agentName = displayInfo.displayName;

  // Sync block prop changes (from block_update patch)
  useEffect(() => {
    setStatus(block.streamStatus);
  }, [block.streamStatus]);

  // "已中断" 仅在历史加载时判断：组件首次 mount 时如果 streamKey 为空且 status=running，
  // 等待一小段时间让 block_update 到达。如果一直没到才标记中断。
  const [waitedForKey, setWaitedForKey] = useState(false);
  useEffect(() => {
    if (block.streamKey || status !== 'running') return;
    const timer = setTimeout(() => setWaitedForKey(true), 3000);
    return () => clearTimeout(timer);
  }, [block.streamKey, status]);

  const isInterrupted = status === 'running' && !block.streamKey && waitedForKey;
  const expanded = previewEntry?.open === true;

  const setExpanded = useCallback((next: boolean) => {
    const state = useStore.getState();
    if (next) {
      state.openSubagentPreview(block.taskId, block.streamKey || null);
    } else {
      state.closeSubagentPreview(block.taskId);
    }
  }, [block.streamKey, block.taskId]);

  useEffect(() => {
    if (block.streamKey) {
      useStore.getState().setSubagentPreviewSessionPath(block.taskId, block.streamKey);
    }
  }, [block.streamKey, block.taskId]);

  useEffect(() => subscribeChatCardNavigation((request) => {
    if (request.kind !== 'subagent' || !request.ids.includes(block.taskId)) return false;
    setExpanded(true);
    window.requestAnimationFrame(() => {
      rootRef.current?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    });
    return true;
  }), [block.taskId, setExpanded]);

  const handleAbort = useCallback(async () => {
    if (!currentSessionPath) return;
    try {
      await stopSubagentProcess({
        sessionId: currentSessionId,
        sessionPath: currentSessionPath,
        taskId: block.taskId,
      });
    } catch {
      useStore.getState().addToast(
        (window.t ?? ((key: string) => key))('rightWorkspace.process.stopFailed'),
        'error',
      );
    }
  }, [block.taskId, currentSessionId, currentSessionPath]);

  const displayLabel = block.label || block.reuseInstance || null;
  const statusLabel = isInterrupted
    ? t('subagent.status.interrupted')
    : status === 'aborted'
      ? t('subagent.status.aborted')
      : status === 'done'
        ? t('subagent.status.done')
        : status === 'failed'
          ? t('subagent.status.failed')
          : t('subagent.status.dispatched');
  const statusTone: ChatResourceCardStatusTone = status === 'done'
    ? 'success'
    : status === 'failed'
      ? 'danger'
      : status === 'running' && !isInterrupted
        ? 'accent'
        : 'muted';

  return (
    <div ref={rootRef} data-subagent-chat-card={block.taskId}>
      <ChatResourceCard
      variant="task"
      compact
      className={`${styles.subagentResourceCard} ${styles[`subagent-${status}`]}`}
      icon={(
        <AgentAvatar
          info={displayInfo}
          className={styles.subagentAvatar}
          alt={agentName}
        />
      )}
      title={agentName}
      titleMeta={displayLabel ? `· ${displayLabel}` : undefined}
      subtitle={block.taskTitle}
      statusLabel={statusLabel}
      statusTone={statusTone}
      expandable
      expanded={expanded}
      onToggle={() => setExpanded(!expanded)}
      ariaLabel={`${agentName} ${block.taskTitle}`}
      actionSlot={status === 'running' && !isInterrupted && (
        <button
          type="button"
          className={styles.subagentAbortBtn}
          onClick={(event) => {
            event.stopPropagation();
            void handleAbort();
          }}
          title={t('subagentAbort')}
        >
          ✕
        </button>
      )}
      >
        <div ref={scrollRef} className={styles.subagentEmbeddedPreview}>
          <SubagentSessionPreview
            taskId={block.taskId}
            sessionId={block.sessionId}
            sessionPath={block.streamKey || null}
            agentId={agentId}
            streamStatus={status}
            summary={block.summary}
            scrollContainerRef={scrollRef}
          />
        </div>
      </ChatResourceCard>
    </div>
  );
});
