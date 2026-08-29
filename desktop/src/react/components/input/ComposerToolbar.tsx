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
  onAttach: () => void;
  slashBtnRef: RefObject<HTMLButtonElement | null>;
  onSlashToggle: () => void;
  /** 知识库引用按钮的会话键（sessionPath；pending 新会话为 HOME_DRAFT_KEY；null/缺省禁用） */
  knowledgeRefSessionKey?: string | null;
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
  showAudioInput: boolean;
  audioRecordingActive: boolean;
  audioRecordingBusy: boolean;
  onAudioToggle: () => void;
}

/** 输入卡片下方的 Composer 工具栏：新建聊天 / 附件 / Slash / 权限模式 / Context / Thinking / 模型 / 语音 */
export const ComposerToolbar = memo(function ComposerToolbar(props: Props) {
  const {
    t, onNewSession, onAttach, slashBtnRef, onSlashToggle,
    knowledgeRefSessionKey,
    permissionMode, onPermissionModeChange, planModeLocked,
    showThinking, thinkingLevel, onThinkingChange, availableThinkingLevels,
    models, sessionModel, isStreaming,
    showAudioInput, audioRecordingActive, audioRecordingBusy, onAudioToggle,
  } = props;

  return (
    <div className={styles['composer-toolbar']}>
      <div className={styles['composer-toolbar-group']}>
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
        <PlanModeButton mode={permissionMode} onChange={onPermissionModeChange} locked={planModeLocked} />
        <ContextRing />
      </div>
      <div className={`${styles['composer-toolbar-group']} ${styles['composer-toolbar-right']}`}>
        {showThinking ? (
          <div className={styles['model-split-control']}>
            <ThinkingLevelButton level={thinkingLevel} onChange={onThinkingChange} availableLevels={availableThinkingLevels} />
            <ModelSelector models={models} sessionModel={sessionModel} isStreaming={isStreaming} />
          </div>
        ) : (
          <ModelSelector models={models} sessionModel={sessionModel} isStreaming={isStreaming} />
        )}
        {showAudioInput && (
          <button
            type="button"
            className={`${styles['audio-record-btn']}${audioRecordingActive ? ` ${styles['is-recording']}` : ''}`}
            title={t(audioRecordingActive ? 'input.stopRecording' : 'input.recordAudio')}
            aria-label={t(audioRecordingActive ? 'input.stopRecording' : 'input.recordAudio')}
            aria-pressed={audioRecordingActive}
            disabled={audioRecordingBusy}
            onClick={onAudioToggle}
          >
            {audioRecordingActive ? (
              <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                <rect x="7" y="7" width="10" height="10" rx="2" />
              </svg>
            ) : (
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3Z" />
                <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                <path d="M12 19v3" />
              </svg>
            )}
          </button>
        )}
      </div>
    </div>
  );
});
