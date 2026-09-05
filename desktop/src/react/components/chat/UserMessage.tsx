/**
 * UserMessage — 用户消息气泡
 */

import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { MarkdownContent } from './MarkdownContent';
import { MessageFooterActions, formatMessageTime, type MessageFooterAction } from './MessageFooterActions';
import { useMessageFooterActions } from './MessageActions';
import { AttachmentChip } from '../shared/AttachmentChip';
import { AudioAttachmentChip } from '../shared/AudioAttachmentChip';
import { FileKindIcon } from '../shared/FileKindIcon';
import { FolderIcon } from '../shared/FolderIcon';
import type { ChatMessage, UserAttachment, DeskContext } from '../../stores/chat-types';
import type { KnowledgeRetrievalStats } from '../../../../../shared/knowledge-refs.ts';
import { useStore } from '../../stores';
import { selectSelectedIdsBySession } from '../../stores/session-selectors';
import { extractSelectedTexts } from '../../utils/message-text';
import { openFilePreview } from '../../utils/file-preview';
import { buildFileRefId, isImageOrSvgExt, extOfName, kindOfFileName } from '../../utils/file-kind';
import { getUserAttachmentImageSrc, getUserAttachmentVideoPosterSrc } from '../../utils/user-attachment-media';
import { AgentAvatar, resolveAgentDisplayInfo } from '../../utils/agent-display';
import { openMediaViewerForRef } from '../../utils/open-media-viewer';
import { useDeferredHistoryContent } from '../../hooks/use-deferred-history-content';
import {
  retrySessionTurn,
  type ForkedSessionHandler,
  type SessionNodeTarget,
} from '../../stores/message-turn-actions';
import { AgentReviewCard } from './AgentReviewCard';
import { AgentReviewRequestCard } from './AgentReviewRequestCard';
import { useSessionNodeActions } from './SessionNodeActions';
import styles from './Chat.module.css';
import badgeStyles from '../input/SkillBadgeView.module.css';

const lazyScreenshot = () => import('../../utils/screenshot').then(m => m.takeScreenshot);

interface Props {
  message: ChatMessage;
  showAvatar: boolean;
  sessionPath: string;
  readOnly?: boolean;
  hideIdentity?: boolean;
  userIdentity?: { name?: string | null; avatarUrl?: string | null };
  viewerIdentity: { name: string; avatarUrl: string | null };
  isStreaming: boolean;
  isSelected: boolean;
  isLatestUserMessage?: boolean;
  onForkCreated?: ForkedSessionHandler;
  messageRef?: (element: HTMLDivElement | null) => void;
}

export const UserMessage = memo(function UserMessage({
  message,
  showAvatar,
  sessionPath,
  readOnly = false,
  hideIdentity = false,
  userIdentity,
  viewerIdentity,
  isStreaming,
  isSelected,
  isLatestUserMessage = false,
  onForkCreated,
  messageRef,
}: Props) {
  const t = window.t ?? ((p: string) => p);
  const storeUserName = viewerIdentity.name;
  const userName = userIdentity?.name || storeUserName;
  const displayAvatarUrl = userIdentity ? (userIdentity.avatarUrl || null) : viewerIdentity.avatarUrl;
  const userDisplayInfo = useMemo(() => resolveAgentDisplayInfo({
    id: 'user',
    agents: [],
    userName,
    userAvatarUrl: displayAvatarUrl,
  }), [userName, displayAvatarUrl]);

  const [copied, setCopied] = useState(false);
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState(message.text || '');
  const [editBusy, setEditBusy] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    if (!editing) setEditValue(message.text || '');
  }, [editing, message.text]);

  useEffect(() => {
    if (!editing) return;
    const el = textareaRef.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
  }, [editing]);

  const handleCopy = useCallback(() => {
    const ids = selectSelectedIdsBySession(useStore.getState(), sessionPath);
    const text = ids.length > 0
      ? extractSelectedTexts(sessionPath, ids)
      : (message.text || '');
    if (!text) return;
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }).catch(() => {});
  }, [message.text, sessionPath]);

  const handleScreenshot = useCallback(async () => {
    const fn = await lazyScreenshot();
    fn(message.id, sessionPath);
  }, [message.id, sessionPath]);

  const isReviewTurn = !!message.agentReview || !!message.agentReviewRequest;
  const turnTarget = useMemo<SessionNodeTarget | null>(() => (
    message.sourceEntryId
      ? { role: 'user', entryId: message.sourceEntryId }
      : null
  ), [message.sourceEntryId]);
  const { actions: nodeActions, busy: nodeActionBusy } = useSessionNodeActions({
    sessionPath,
    target: readOnly ? null : turnTarget,
    retryMessage: message,
    onForkCreated,
    disabled: isStreaming,
  });
  const busy = editBusy || nodeActionBusy;

  const handleEdit = useCallback(() => {
    if (busy || isStreaming) return;
    setEditValue(message.text || '');
    setEditing(true);
  }, [busy, isStreaming, message.text]);

  const handleCancelEdit = useCallback(() => {
    if (busy) return;
    setEditing(false);
    setEditValue(message.text || '');
  }, [busy, message.text]);

  const handleConfirmEdit = useCallback(async () => {
    const nextText = editValue.trim();
    if (!nextText || busy || isStreaming) return;
    setEditBusy(true);
    try {
      if (!turnTarget) return;
      const ok = await retrySessionTurn(
        sessionPath,
        turnTarget,
        { message, replacementText: nextText },
      );
      if (ok) setEditing(false);
    } finally {
      setEditBusy(false);
    }
  }, [busy, editValue, isStreaming, message, sessionPath, turnTarget]);

  // Retry and fork preserve the recorded review envelope. Inline text editing remains
  // unavailable because changing only its text would no longer match that snapshot.
  const canEdit = !readOnly && !isReviewTurn && isLatestUserMessage && !!turnTarget;
  const timeText = formatMessageTime(message.timestamp);
  const editingActions: MessageFooterAction[] = useMemo(() => [
    {
      id: 'cancel',
      title: t('common.cancel'),
      icon: <XIcon />,
      onClick: () => handleCancelEdit(),
      disabled: busy,
    },
    {
      id: 'confirm',
      title: t('common.confirm'),
      icon: <CheckIcon />,
      onClick: () => { void handleConfirmEdit(); },
      disabled: busy || !editValue.trim(),
    },
  ], [busy, editValue, handleCancelEdit, handleConfirmEdit, t]);
  const standardMessageActions = useMessageFooterActions({
    messageId: message.id,
    sessionPath,
    onCopy: handleCopy,
    onScreenshot: () => { void handleScreenshot(); },
    copied,
    isStreaming: isStreaming || busy,
  });
  const messageActions = readOnly || editing ? [] : standardMessageActions;
  const editActions: MessageFooterAction[] = useMemo(() => canEdit ? [
    {
      id: 'edit',
      title: t('common.edit'),
      icon: <EditIcon />,
      onClick: () => handleEdit(),
      disabled: isStreaming || busy,
    },
  ] : [], [busy, canEdit, handleEdit, isStreaming, t]);
  const footerActions = editing ? editingActions : [...nodeActions, ...editActions];
  const hasSkillBadges = !!message.skills?.length;
  const hasTextBubble = editing || !!message.textHtml || hasSkillBadges;

  return (
    <div className={`${styles.messageGroup} ${styles.messageGroupUser}${isSelected ? ` ${styles.messageGroupSelected}` : ''}`}
         ref={messageRef}
         data-message-id={message.id}>
      {showAvatar && !hideIdentity && (
        <div className={`${styles.avatarRow} ${styles.avatarRowUser}`}>
          <span className={styles.avatarName}>{userName}</span>
          <AgentAvatar
            info={userDisplayInfo}
            className={`${styles.avatar} ${styles.userAvatar}`}
            alt={userName}
          />
        </div>
      )}
      {message.quotedText && (
        <div className={styles.userAttachments}>
          <AttachmentChip
            icon={<GridIcon />}
            name={message.quotedText}
          />
        </div>
      )}
      {message.knowledgeRefs && message.knowledgeRefs.notebookIds.length > 0 && (
        <UserKnowledgeMeta knowledgeRefs={message.knowledgeRefs} retrieval={message.knowledgeRetrieval} />
      )}
      {message.attachments && message.attachments.length > 0 && (
        <UserAttachmentsView
          attachments={message.attachments}
          deskContext={message.deskContext}
          sessionPath={sessionPath}
          messageId={message.id}
        />
      )}
      {hasTextBubble && (
        <div className={`${styles.message} ${styles.messageUser}${editing ? ` ${styles.messageUserEditing}` : ''}`}>
          {message.skills && message.skills.length > 0 && message.skills.map(skillName => (
            <span key={skillName} className={badgeStyles.badge} style={{ cursor: 'default' }}>
              <svg className={badgeStyles.icon} width="13" height="13" viewBox="0 0 16 16" fill="none"
                stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round">
                <path d="M8 1 L9.5 6 L15 8 L9.5 10 L8 15 L6.5 10 L1 8 L6.5 6 Z" />
              </svg>
              <span className={badgeStyles.name}>{skillName}</span>
            </span>
          ))}
          {editing ? (
            <textarea
              ref={textareaRef}
              className={styles.userEditTextarea}
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              onKeyDown={(e) => {
                if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                  e.preventDefault();
                  void handleConfirmEdit();
                }
                if (e.key === 'Escape') {
                  e.preventDefault();
                  handleCancelEdit();
                }
              }}
              disabled={busy}
              spellCheck={false}
            />
          ) : (
            message.textHtml && <MarkdownContent html={message.textHtml} linkContext={{ origin: 'session', sessionPath, messageId: message.id }} />
          )}
        </div>
      )}
      {message.agentReview && <AgentReviewCard review={message.agentReview} />}
      {message.agentReviewRequest && <AgentReviewRequestCard request={message.agentReviewRequest} />}
      {(timeText || messageActions.length > 0 || footerActions.length > 0) && (
        <MessageFooterActions
          align="right"
          timeText={timeText}
          leadingActions={footerActions}
          visible={editing}
          actions={messageActions}
          testId="user-message-footer-actions"
        />
      )}
    </div>
  );
});

// ── 知识库元信息行 ──

/**
 * 用户消息上方的知识库引用元信息：一行 muted 小字，只显示来源与模式
 * （知识库 · 笔记本）；旧消息保留原模式标签，整体不可用时追加提示。
 * 检索统计（块数/注入数/tokens/超预算分片）不再上屏——面向用户的成败信号
 * 由蒸馏进度胶囊与知识检索折叠卡承载。模式 hint 与拆解降级原因收进
 * title tooltip，不占行宽。
 */
const UserKnowledgeMeta = memo(function UserKnowledgeMeta({
  knowledgeRefs,
  retrieval,
}: {
  knowledgeRefs: NonNullable<ChatMessage['knowledgeRefs']>;
  retrieval?: KnowledgeRetrievalStats;
}) {
  const t = window.t ?? ((p: string) => p);
  const locale = useStore(s => s.locale);
  const names = knowledgeRefs.notebookIds.map(notebookId =>
    knowledgeRefs.notebooks?.find(nb => nb.id === notebookId)?.name || notebookId);
  // 名称缺失回退 id；CJK 语言用顿号枚举，其余用逗号（保持单行紧凑，不走 ListFormat 的「和」连接）。
  const separator = /^(zh|ja)([-_]|$)/.test(locale || '') ? '、' : ', ';
  const nameList = names.join(separator);
  // 统一聊天不显示模式标签；旧消息保留当时的模式。
  const modeLabelKey = knowledgeRefs.mode === 'fast'
    ? 'chat.knowledgeMetaModeFast'
    : knowledgeRefs.mode === 'detailed'
      ? 'chat.knowledgeMetaModeDetailed'
      : knowledgeRefs.mode === 'qa'
        ? 'chat.knowledgeMetaModeQa'
        : 'chat.knowledgeMetaModeAssist';
  const modeHintKey = knowledgeRefs.mode === 'fast'
    ? 'input.knowledgeModeFastHint'
    : knowledgeRefs.mode === 'detailed'
      ? 'input.knowledgeModeDetailedHint'
      : knowledgeRefs.mode === 'qa'
        ? 'input.knowledgeModeQaHint'
        : 'input.knowledgeModeAssistHint';
  const parts = [t('chat.knowledgeMetaLabel'), nameList];
  if (knowledgeRefs.mode !== 'auto') parts.push(t(modeLabelKey));
  if (retrieval?.unavailableReason) {
    parts.push(t('chat.knowledgeMetaUnavailable'));
  }
  const modeHint = knowledgeRefs.mode === 'auto' ? '' : t(modeHintKey);
  const degradeTitle = retrieval?.degraded
    ? t('chat.knowledgeMetaDegradedTitle', { reason: retrieval.degradeReason || '' })
    : '';
  return (
    <div className={styles.userKnowledgeMeta} title={degradeTitle ? `${modeHint}\n${degradeTitle}` : modeHint}>
      {parts.join(' · ')}
    </div>
  );
});

// ── 附件区 ──

const UserAttachmentsView = memo(function UserAttachmentsView({ attachments, deskContext, sessionPath, messageId }: {
  attachments: UserAttachment[];
  deskContext?: DeskContext | null;
  sessionPath: string;
  messageId: string;
}) {
  // 扩展名识别统一走中心表 EXT_TO_KIND；禁止维护私有 IMAGE_EXTS 表。
  const isImage = useCallback((att: UserAttachment) => {
    return isImageOrSvgExt(extOfName(att.name));
  }, []);

  const t = window.t ?? ((p: string) => p);

  return (
    <div className={styles.userAttachments}>
      {attachments.map((att, i) => {
        const expired = att.status === 'expired';
        const expiredLabel = t('chat.fileExpired');
        const imageSrc = !expired && isImage(att) ? getUserAttachmentImageSrc(att) : null;
        const kind = att.isDir ? 'directory' : kindOfFileName(att.name || att.path, att.mimeType);
        if (!expired && att.deferred && isImage(att)) {
          return (
            <DeferredUserImageAttachment
              key={att.fileId || att.path || att.name || `att-${i}`}
              attachment={att}
              sessionPath={sessionPath}
              messageId={messageId}
            />
          );
        }
        if (!expired && kind === 'audio') {
          const isVoiceInput = att.presentation === 'voice-input';
          const transcriptText = isVoiceInput && att.transcription?.status === 'ready'
            ? att.transcription.text?.trim()
            : '';
          if (isVoiceInput) {
            return (
              <div key={att.fileId || att.path || att.name || `att-${i}`} className={styles.voiceInputCard}>
                {transcriptText && <div className={styles.voiceInputTranscript}>{transcriptText}</div>}
                <AudioAttachmentChip
                  file={{
                    path: att.path,
                    name: att.name,
                    base64Data: att.base64Data,
                    mimeType: att.mimeType,
                    waveform: att.waveform,
                  }}
                  showName={false}
                  className={styles.voiceInputAudioStrip}
                  waveform={att.waveform}
                />
              </div>
            );
          }
          return (
            <AudioAttachmentChip
              key={att.fileId || att.path || att.name || `att-${i}`}
              file={{
                path: att.path,
                name: att.name,
                base64Data: att.base64Data,
                mimeType: att.mimeType,
                waveform: att.waveform,
              }}
              showName={att.presentation !== 'voice-input'}
              waveform={att.waveform}
            />
          );
        }
        // 视频附件：首帧海报 + 播放角标，点击进 MediaViewer 全屏播放
        //（此前走兜底文件胶囊，无任何点击入口——视频「看得见却点不开」）。
        if (!expired && kind === 'video') {
          return (
            <UserVideoAttachmentCard
              key={att.fileId || att.path || att.name || `att-${i}`}
              attachment={att}
              sessionPath={sessionPath}
              messageId={messageId}
            />
          );
        }
        if (imageSrc) {
          return (
            <div key={att.name || `att-${i}`} className={styles.attachImageWrap}>
              <img
                className={styles.attachImage}
                src={imageSrc}
                alt={att.name}
                loading="lazy"
                onClick={(e) => {
                  e.stopPropagation();
                  const ext = att.name.split('.').pop()?.toLowerCase() || '';
                  openFilePreview(att.path, att.name, ext, {
                    origin: 'session',
                    sessionPath,
                    messageId,
                  });
                }}
                style={{ cursor: 'default' }}
              />
              {att.visionAuxiliary && (
                <div className={styles.visionAuxiliaryLabel}>
                  {t('chat.visionAuxiliary')}
                </div>
              )}
            </div>
          );
        }
        return (
          <AttachmentChip
            key={att.fileId || att.path || att.name || `att-${i}`}
            icon={att.isDir ? <FolderIcon /> : <FileKindIcon kind={kindOfFileName(att.name || att.path, att.mimeType)} size={14} />}
            name={expired ? `${att.name} · ${expiredLabel}` : att.name}
            variant={expired ? 'expired' : 'normal'}
          />
        );
      })}
      {deskContext && (
        <AttachmentChip
          icon={<FolderIcon />}
          name={`${t('sidebar.jian')} (${deskContext.fileCount})`}
        />
      )}
    </div>
  );
});

/** 视频附件卡：首帧海报 + 播放角标；海报解码失败（HEVC 等编码）回退文件图标，
 * 卡片本身仍可点击进 MediaViewer（那里有「用系统播放器打开」逃生门）。 */
const UserVideoAttachmentCard = memo(function UserVideoAttachmentCard({
  attachment: att,
  sessionPath,
  messageId,
}: {
  attachment: UserAttachment;
  sessionPath: string;
  messageId: string;
}) {
  const [posterFailed, setPosterFailed] = useState(false);
  const posterSrc = getUserAttachmentVideoPosterSrc(att);
  return (
    <div
      className={styles.attachVideoWrap}
      role="button"
      aria-label={att.name}
      onClick={(e) => {
        e.stopPropagation();
        const ext = att.name.split('.').pop()?.toLowerCase() || '';
        openFilePreview(att.path, att.name, ext, {
          origin: 'session',
          sessionPath,
          messageId,
        });
      }}
    >
      <div className={styles.attachVideoPosterBox}>
        {posterSrc && !posterFailed ? (
          <video
            className={styles.attachVideoPoster}
            src={posterSrc}
            preload="metadata"
            muted
            playsInline
            aria-hidden
            onError={() => setPosterFailed(true)}
          />
        ) : (
          <div className={styles.attachVideoPosterFallback}>
            <FileKindIcon kind="video" size={22} />
          </div>
        )}
        <span className={styles.attachVideoPlayBadge} aria-hidden>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="#fff" aria-hidden="true">
            <path d="M8 5v14l11-7z" />
          </svg>
        </span>
      </div>
      <span className={styles.attachVideoName} title={att.name}>{att.name}</span>
    </div>
  );
});

const DeferredUserImageAttachment = memo(function DeferredUserImageAttachment({
  attachment,
  sessionPath,
  messageId,
}: {
  attachment: UserAttachment;
  sessionPath: string;
  messageId: string;
}) {
  const [requested, setRequested] = useState(false);
  const loaded = useDeferredHistoryContent(sessionPath, attachment.deferred, requested);
  const base64 = loaded.data?.content || attachment.base64Data || '';
  const mimeType = loaded.data?.mimeType || attachment.mimeType || 'image/png';
  const openImage = useCallback((content: string, mime: string) => {
    openMediaViewerForRef({
      id: buildFileRefId({
        source: 'session-attachment',
        sessionPath,
        messageId,
        path: attachment.path,
      }),
      kind: 'image',
      source: 'session-attachment',
      name: attachment.name,
      path: attachment.path,
      mime,
      sessionMessageId: messageId,
      inlineData: { base64: content, mimeType: mime },
    }, { origin: 'session', sessionPath });
  }, [attachment.name, attachment.path, messageId, sessionPath]);
  const handleClick = () => {
    if (!base64) {
      setRequested(true);
      return;
    }
    openImage(base64, mimeType);
  };

  useEffect(() => {
    if (!requested) return;
    if (loaded.data) {
      openImage(loaded.data.content, loaded.data.mimeType || mimeType);
      setRequested(false);
    } else if (loaded.error) {
      setRequested(false);
    }
  }, [loaded.data, loaded.error, mimeType, openImage, requested]);

  if (base64) {
    return (
      <div className={styles.attachImageWrap}>
        <img
          className={styles.attachImage}
          src={`data:${mimeType};base64,${base64}`}
          alt={attachment.name}
          loading="lazy"
          onClick={handleClick}
          style={{ cursor: 'default' }}
        />
      </div>
    );
  }

  return (
    <button type="button" className={styles.deferredAttachmentButton} onClick={handleClick}>
      <FileKindIcon kind="image" size={14} />
      <span>{attachment.name}</span>
    </button>
  );
});

// ── Icons ──

function GridIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <line x1="6" y1="4" x2="6" y2="20" />
      <line x1="18" y1="4" x2="18" y2="20" />
      <line x1="6" y1="8" x2="18" y2="8" />
      <line x1="6" y1="16" x2="18" y2="16" />
    </svg>
  );
}

function EditIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

function XIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d="M18 6 6 18" />
      <path d="m6 6 12 12" />
    </svg>
  );
}
