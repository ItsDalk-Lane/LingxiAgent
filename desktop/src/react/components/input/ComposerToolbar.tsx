import { memo, type RefObject } from 'react';
import { PlanModeButton, type PermissionMode } from './PlanModeButton';
import { ContextRing } from './ContextRing';
import { ThinkingLevelButton } from './ThinkingLevelButton';
import { ModelSelector } from './ModelSelector';
import { KnowledgeReferenceButton } from './KnowledgeReferenceButton';
import type { ThinkingLevel } from '../../stores/model-slice';
import type { Model } from '../../types';
import type { SessionModel } from '../../stores/chat-types';
import styles from './InputArea.module.css';

interface Props {
  t: (key: string) => string;
  // 左侧工具按钮
  onNewSession: () => void;
  /**
   * 侧边对话面板的输入区传 true：本工具栏隐藏「新建聊天」入口（它的语义是
   * 离开当前会话去开新草稿，与侧栏「在旁支里继续追问」冲突）。
   */
  sideChatScope?: boolean;
  onAttach: () => void;
  slashBtnRef: RefObject<HTMLButtonElement | null>;
  onSlashToggle: () => void;
  /** 知识库引用按钮的会话键（sessionPath；pending 新会话为 HOME_DRAFT_KEY；null/缺省禁用） */
  knowledgeRefSessionKey?: string | null;
  /** 本输入区归属的会话 path（主聊天页 = 当前会话；侧边面板 = 侧边会话）。 */
  sessionPath?: string | null;
  /** 是否显示会话记忆开关（侧边对话面板；主聊天页沿用 Welcome 页的入口）。 */
  showMemoryToggle?: boolean;
  memoryEnabled?: boolean;
  onMemoryChange?: (enabled: boolean) => void;
  /**
   * 模型选择器显式绑定的会话（侧边对话面板传入侧边会话 path）。
   * 省略时模型选择器沿用全局 currentSessionPath（主聊天页既有语义）。
   */
  sessionModelScopePath?: string | null;
  permissionMode: PermissionMode;
  onPermissionModeChange: (v: PermissionMode) => void;
  planModeLocked: boolean;
  // 右侧控制
  showThinking: boolean;
  thinkingLevel: ThinkingLevel;
  onThinkingChange: (level: ThinkingLevel) => void;
  availableThinkingLevels: ThinkingLevel[];
  models: Model[];
  sessionModel?: SessionModel;
  isStreaming: boolean;
}

/** 输入卡片下方的 Composer 工具栏：新建聊天 / 附件 / Slash / 权限模式 / Context / Thinking / 模型 */
export const ComposerToolbar = memo(function ComposerToolbar(props: Props) {
  const {
    t, onNewSession, sideChatScope = false, onAttach, slashBtnRef, onSlashToggle,
    knowledgeRefSessionKey, sessionPath,
    showMemoryToggle = false, memoryEnabled = true, onMemoryChange,
    sessionModelScopePath,
    permissionMode, onPermissionModeChange, planModeLocked,
    showThinking, thinkingLevel, onThinkingChange, availableThinkingLevels,
    models, sessionModel, isStreaming,
  } = props;

  return (
    <div className={styles['composer-toolbar']}>
      <div className={styles['composer-toolbar-group']}>
        {!sideChatScope && (
          <button
            className={styles['attach-btn']}
            title={t('sidebar.newChat')}
            onClick={onNewSession}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
              <path d="M9 10h6" />
              <path d="M12 7v6" />
            </svg>
          </button>
        )}
        <button
          className={styles['attach-btn']}
          title={t('input.attachFiles')}
          onClick={onAttach}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" />
          </svg>
        </button>
        <button
          ref={slashBtnRef}
          className={styles['attach-btn']}
          title={t('input.commandMenu')}
          onClick={onSlashToggle}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 2L14 10L22 12L14 14L12 22L10 14L2 12L10 10Z" />
          </svg>
        </button>
        <KnowledgeReferenceButton sessionKey={knowledgeRefSessionKey ?? null} />
        <PlanModeButton mode={permissionMode} onChange={onPermissionModeChange} locked={planModeLocked} sessionPath={sessionPath} />
        {showMemoryToggle && (
          <button
            type="button"
            className={`${styles['attach-btn']}${memoryEnabled ? ` ${styles['is-active']}` : ''}`}
            data-memory-enabled={memoryEnabled ? 'true' : 'false'}
            title={memoryEnabled ? t('welcome.memoryOn') : t('welcome.memoryOff')}
            aria-label={memoryEnabled ? t('welcome.memoryOn') : t('welcome.memoryOff')}
            aria-pressed={memoryEnabled}
            onClick={() => onMemoryChange?.(!memoryEnabled)}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M12 4.5c-3.6 0-6.5 2.6-6.5 6 0 1.5.6 2.9 1.6 4 .3.3.4.7.4 1.1v1.2c0 .9.7 1.7 1.7 1.7h5.6c.9 0 1.7-.8 1.7-1.7v-1.2c0-.4.1-.8.4-1.1 1-1.1 1.6-2.5 1.6-4 0-3.4-2.9-6-6.5-6Z" />
              <path d="M9.5 21h5" />
            </svg>
          </button>
        )}
        <ContextRing />
      </div>
      <div className={`${styles['composer-toolbar-group']} ${styles['composer-toolbar-right']}`}>
        {showThinking ? (
          <div className={styles['model-split-control']}>
            <ThinkingLevelButton level={thinkingLevel} onChange={onThinkingChange} availableLevels={availableThinkingLevels} />
            <ModelSelector models={models} sessionModel={sessionModel} isStreaming={isStreaming} sessionPath={sessionModelScopePath} />
          </div>
        ) : (
          <ModelSelector models={models} sessionModel={sessionModel} isStreaming={isStreaming} sessionPath={sessionModelScopePath} />
        )}
      </div>
    </div>
  );
});
