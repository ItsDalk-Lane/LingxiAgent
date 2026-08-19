import { memo, useCallback, useEffect, useId, useMemo, useState } from 'react';
import { Collapse } from '@/ui';
import { AgentAvatar, type AgentDisplayInfo } from '../../utils/agent-display';
import { AssistantBlockList } from './AssistantBlockList';
import { MessageFooterActions, formatMessageTime } from './MessageFooterActions';
import { buildProcessFoldSummary, type ProcessFoldRenderItem } from './process-fold';
import { useSessionNodeActions } from './SessionNodeActions';
import type {
  ForkedSessionHandler,
  SessionNodeTarget,
} from '../../stores/message-turn-actions';
import type { ChatMessage } from '../../stores/chat-types';
import styles from './Chat.module.css';
import { subscribeChatCardNavigation } from '../../services/chat-card-navigation';

interface Props {
  group: ProcessFoldRenderItem;
  showAvatar: boolean;
  sessionPath: string;
  agentId?: string | null;
  readOnly: boolean;
  turnCompletionAssistantIndexes?: ReadonlySet<number>;
  assistantTurnSelectionIdsByCompletionIndex?: ReadonlyMap<number, readonly string[]>;
  assistantTurnTargetsByCompletionIndex?: ReadonlyMap<number, SessionNodeTarget>;
  assistantTurnRetryMessagesByCompletionIndex?: ReadonlyMap<number, ChatMessage>;
  assistantSkillPromptsByIndex?: ReadonlyMap<number, string>;
  completionTimePersistent?: boolean;
  agentDisplay: AgentDisplayInfo & { yuan: string };
  isStreaming: boolean;
  selectedIds: readonly string[];
  registerMessageElement?: (messageId: string, element: HTMLDivElement | null) => void;
  onForkCreated?: ForkedSessionHandler;
}

export const ProcessFoldBlock = memo(function ProcessFoldBlock({
  group,
  showAvatar,
  sessionPath,
  agentId,
  readOnly,
  turnCompletionAssistantIndexes,
  assistantTurnSelectionIdsByCompletionIndex,
  assistantTurnTargetsByCompletionIndex,
  assistantTurnRetryMessagesByCompletionIndex,
  assistantSkillPromptsByIndex,
  completionTimePersistent = false,
  agentDisplay,
  isStreaming,
  selectedIds,
  registerMessageElement,
  onForkCreated,
}: Props) {
  // ProcessRegion：live 模式不折叠、不显示 summary（任务书 §二十五/§三十一）；
  // settled 模式才显示 summary 并默认折叠。
  const isLive = group.mode === 'live';
  const [open, setOpen] = useState(isLive ? true : !group.defaultCollapsed);
  const panelId = useId();
  const t = window.t ?? ((p: string) => p);

  const displayName = agentDisplay.displayName;
  const displayInfo = agentDisplay;
  const summary = useMemo(
    () => buildProcessFoldSummary(
      group.stats,
      displayName,
      (key, vars) => String(t(key, vars as Record<string, string | number> | undefined)),
    ),
    [displayName, group.stats, t],
  );

  const toggle = useCallback(() => setOpen(value => !value), []);
  useEffect(() => {
    setOpen(isLive ? true : !group.defaultCollapsed);
  }, [group.defaultCollapsed, group.id, isLive]);
  useEffect(() => subscribeChatCardNavigation((request) => {
    const anchors = request.kind === 'terminal'
      ? group.navigationAnchors.terminal
      : group.navigationAnchors.subagent;
    if (!request.ids.some((id) => anchors.includes(id))) return false;
    // 这里只展开父级，不消费请求；子卡挂载后会接走 pending 请求并精确滚动。
    setOpen(true);
    return false;
  }), [group.navigationAnchors]);
  const registerRefElement = useCallback((messageId: string) => (
    (element: HTMLDivElement | null) => registerMessageElement?.(messageId, element)
  ), [registerMessageElement]);
  // 完成状态/操作取该轮最后一个源消息（与 turnCompletion 索引对齐）。
  const lastRef = group.refs[group.refs.length - 1];
  const turnCompletionEntry = group.ownsTurnCompletion && turnCompletionAssistantIndexes && lastRef
    && turnCompletionAssistantIndexes.has(lastRef.originalIndex)
    ? lastRef
    : null;
  const completionTimeText = formatMessageTime(turnCompletionEntry?.timestamp ?? undefined) || null;
  const completionTarget = turnCompletionEntry
    ? assistantTurnTargetsByCompletionIndex?.get(turnCompletionEntry.originalIndex) ?? null
    : null;
  const { actions: completionActions } = useSessionNodeActions({
    sessionPath,
    target: readOnly || !turnCompletionEntry || isStreaming ? null : completionTarget,
    retryMessage: turnCompletionEntry
      ? assistantTurnRetryMessagesByCompletionIndex?.get(turnCompletionEntry.originalIndex)
      : undefined,
    onForkCreated,
    disabled: isStreaming,
  });

  return (
    <>
      <div
        className={`${styles.messageGroup} ${styles.messageGroupAssistant}`}
        data-process-group-id={group.id}
        data-turn-id={group.turnId}
        data-block-ids={group.blockIds.join(' ')}
      >
        {showAvatar && (
          <div className={styles.avatarRow}>
            <AgentAvatar
              info={displayInfo}
              className={`${styles.avatar} ${styles.hanaAvatar}`}
              alt={displayName}
            />
            <span className={styles.avatarName}>{displayName}</span>
          </div>
        )}
        {!isLive && (
          <div className={`${styles.message} ${styles.messageAssistant} ${styles.processFoldMessage}`}>
            <button
              type="button"
              className={`${styles.processFoldSummary}${open ? ` ${styles.processFoldSummaryOpen}` : ''}`}
              aria-expanded={open}
              aria-controls={panelId}
              onClick={toggle}
            >
              <span className={styles.processFoldTitle}>
                <span className={styles.processFoldTitleText}>{summary}</span>
                <span className={styles.processFoldArrow} aria-hidden="true">›</span>
              </span>
            </button>
          </div>
        )}
        <Collapse open={open} className={styles.processFoldCollapse}>
          <div id={panelId} className={`${styles.message} ${styles.messageAssistant} ${styles.processFoldPanel}`}>
            {group.refs.map((ref) => (
              <div
                key={ref.sourceMessageId}
                ref={ref.registerSourceMessageElement ? registerRefElement(ref.sourceMessageId) : undefined}
              >
                <AssistantBlockList
                  blocks={ref.blocks}
                  agentName={displayName}
                  agentId={agentId}
                  yuan={agentDisplay.yuan}
                  sessionPath={sessionPath}
                  messageId={ref.sourceMessageId}
                  isStreaming={isStreaming}
                  readOnly={readOnly}
                  skillPrompt={assistantSkillPromptsByIndex?.get(ref.originalIndex) ?? null}
                />
              </div>
            ))}
          </div>
        </Collapse>
        {!open && (completionTimeText || completionActions.length > 0) && (
          <MessageFooterActions
            align="left"
            timeText={completionTimeText}
            timePersistent={completionTimePersistent}
            leadingActions={completionActions}
            actions={[]}
            testId="process-fold-completion-actions"
          />
        )}
      </div>
    </>
  );
});
