/** ChannelsPanel — 频道系统入口 + 保留组件（子组件在 ./channels/） */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../stores';
import { sessionScopedValue } from '../stores/session-slice';
import { fetchConfig } from '../hooks/use-config';
import { lingxiFetch } from '../services/service-client';
import { captureConversationRequestScope } from '../services/conversation-request-scope';
import { resolveServerConnection } from '../services/service-client';
import { useI18n } from '../hooks/use-i18n';
import { renderMarkdown } from '../utils/markdown';
import { findOpenToolIndex, toolCallFromStartEvent, toolCallIdFromEvent } from '../utils/tool-call-identity';
import { MarkdownContent } from './chat/MarkdownContent';
import {
  addChannelMember,
  exportCurrentConversation,
  loadChannels,
  loadConversationAgentPhoneSettings,
  openChannel,
  removeChannelMember,
  saveConversationAgentPhoneSettings,
  sendChannelMessage,
} from '../stores/channel-actions';
import { loadMessages } from '../stores/session-actions';
import { subscribeStreamKey } from '../services/stream-key-dispatcher';
import { useContinuousBottomScroll } from '../hooks/use-continuous-bottom-scroll';
import { resolveChannelMember, buildAgentMap, formatChannelTime, MemberAvatar } from './channels/ChannelList';
import type { MemberInfo } from './channels/ChannelList';
import { ChatTranscript } from './chat/ChatTranscript';
import { Button, ContextMenu, type ContextMenuItem } from '../ui';
import type { ChatListItem, ChatMessage, ContentBlock } from '../stores/chat-types';
import type { AgentPhoneActivity, Channel, ChannelTickerStatus, Model } from '../types';
import styles from './channels/Channels.module.css';
import chatStyles from './chat/Chat.module.css';

/* eslint-disable @typescript-eslint/no-explicit-any -- isComposing 等 nativeEvent 字段需 as any */

const CHANNEL_SCROLL_THRESHOLD = 80;
const EMPTY_CHAT_ITEMS: ChatListItem[] = [];
const PHONE_STREAM_MESSAGE_PREFIX = 'agent-phone-stream';
const CHANNEL_COMPOSER_FOCUS_EVENT = 'hana-channel-composer-focus';

function channelMaxScrollTop(el: HTMLElement): number {
  return Math.max(0, el.scrollHeight - el.clientHeight);
}

function setChannelScrollTopInstant(el: HTMLElement, top: number): void {
  const previousScrollBehavior = el.style.scrollBehavior;
  el.style.scrollBehavior = 'auto';
  el.scrollTop = top;
  if (previousScrollBehavior) {
    el.style.scrollBehavior = previousScrollBehavior;
  } else {
    el.style.removeProperty('scroll-behavior');
  }
}

export function requestChannelComposerFocus(channelId: string) {
  if (!channelId || typeof window === 'undefined') return;
  window.dispatchEvent(new CustomEvent(CHANNEL_COMPOSER_FOCUS_EVENT, {
    detail: { channelId },
  }));
}

function resolveDmOwnerId(channel: Channel | undefined, currentAgentId: string | null): string {
  return channel?.isDM ? (channel.dmOwnerId || currentAgentId || '') : (currentAgentId || '');
}

export function ChannelsPanel() {
  const connectionKey = useStore(s => JSON.stringify(resolveServerConnection(s)));
  const currentAgentId = useStore(s => s.currentAgentId);

  useEffect(() => {
    // 切换连接或助手后，缓存与在途结果不能借用上一处的内容。
    useStore.setState({
      channelsEnabled: undefined, channels: [], currentChannel: null,
      channelMessages: [], channelMessageCache: {}, channelMessageCacheDirty: {},
      channelMembers: [], channelAgentActivities: {}, channelTickerStatus: {},
      channelTotalUnread: 0, channelHeaderName: '', channelHeaderMembersText: '',
      channelInfoName: '', channelIsDM: false, channelListError: null,
      channelListLoading: false, channelHistoryError: null, channelHistoryLoading: false,
      channelSettingsError: null, channelSettingsLoading: false,
    });
    if (!resolveServerConnection(useStore.getState())) return;
    const scope = captureConversationRequestScope();
    void fetchConfig({ cacheKey: `${connectionKey}:${currentAgentId || ''}` }).then(config => {
      if (!scope.isCurrent()) return;
      const enabled = config?.channels?.enabled === true;
      useStore.getState().setChannelsEnabled(enabled);
      if (enabled) void loadChannels();
    }).catch(error => {
      if (scope.isCurrent()) useStore.setState({ channelListError: error instanceof Error ? error.message : String(error) });
    });
    return scope.dispose;
  }, [connectionKey, currentAgentId]);

  return null;
}

// ── ChannelMessages — 消息列表

export function ChannelMessages() {
  const { t } = useI18n();
  const messages = useStore(s => s.channelMessages);
  const historyError = useStore(s => s.channelHistoryError);
  const historyLoading = useStore(s => s.channelHistoryLoading);
  const currentChannel = useStore(s => s.currentChannel);
  const channels = useStore(s => s.channels);
  const agents = useStore(s => s.agents);
  const userName = useStore(s => s.userName);
  const userAvatarUrl = useStore(s => s.userAvatarUrl);
  const currentAgentId = useStore(s => s.currentAgentId);
  const agentMap = useMemo(() => buildAgentMap(agents), [agents]);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const isNearBottomRef = useRef(true);
  const previousChannelRef = useRef<string | null>(null);
  const previousLengthRef = useRef(0);
  const [showNewMessages, setShowNewMessages] = useState(false);

  const getScrollContainer = useCallback(() => (
    wrapperRef.current?.closest('.channel-messages') as HTMLElement | null
  ), []);

  const checkNearBottom = useCallback(() => {
    const el = getScrollContainer();
    if (!el) return true;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight <= CHANNEL_SCROLL_THRESHOLD;
    isNearBottomRef.current = near;
    if (near) setShowNewMessages(false);
    return near;
  }, [getScrollContainer]);

  const scrollToBottom = useCallback(() => {
    const el = getScrollContainer();
    if (!el) return;
    setChannelScrollTopInstant(el, channelMaxScrollTop(el));
    isNearBottomRef.current = true;
    setShowNewMessages(false);
  }, [getScrollContainer]);

  useLayoutEffect(() => {
    isNearBottomRef.current = true;
    previousChannelRef.current = null;
    previousLengthRef.current = 0;
    setShowNewMessages(false);
  }, [currentChannel]);

  useEffect(() => {
    const el = getScrollContainer();
    if (!el) return;
    const onScroll = () => { checkNearBottom(); };
    el.addEventListener('scroll', onScroll, { passive: true });
    return () => el.removeEventListener('scroll', onScroll);
  }, [checkNearBottom, getScrollContainer, messages.length]);

  useLayoutEffect(() => {
    const el = getScrollContainer();
    const channelChanged = previousChannelRef.current !== currentChannel;
    const previousLength = previousLengthRef.current;
    const grew = messages.length > previousLength;

    if (el && messages.length > 0) {
      if (channelChanged || previousLength === 0) {
        scrollToBottom();
      } else if (grew) {
        const nearNow = el.scrollHeight - el.scrollTop - el.clientHeight <= CHANNEL_SCROLL_THRESHOLD;
        if (isNearBottomRef.current || nearNow) {
          scrollToBottom();
        } else {
          setShowNewMessages(true);
        }
      }
    }

    previousChannelRef.current = currentChannel;
    previousLengthRef.current = messages.length;
  }, [currentChannel, getScrollContainer, messages.length, scrollToBottom]);

  const feedback = currentChannel && historyError ? (
    <div role="alert" className={styles.channelWelcome}>
      <span>{t('channel.loadFailed')}</span>
      <button type="button" disabled={historyLoading} onClick={() => { void openChannel(currentChannel); }}>{t('channel.retry')}</button>
    </div>
  ) : null;
  if (!currentChannel || messages.length === 0) {
    return feedback || <div className={styles.channelWelcome}>{t(historyLoading ? 'common.loading' : 'channel.noMessages')}</div>;
  }

  const ch = channels.find((c) => c.id === currentChannel);
  const isDM = ch?.isDM ?? false;
  const dmOwnerId = resolveDmOwnerId(ch, currentAgentId);
  let lastSender: string | null = null;

  return (
    <>
      {feedback}
      <div ref={wrapperRef}>
        {messages.map((msg, idx) => {
          const isContinuation = msg.sender === lastSender;
          const senderInfo = resolveChannelMember(msg.sender, userName, userAvatarUrl, agents, isDM ? dmOwnerId : currentAgentId, agentMap);
          const isSelf = senderInfo.isUser || (isDM && msg.sender === dmOwnerId);
          const el = (
            <div
              key={`${msg.timestamp}-${msg.sender}-${idx}`}
              className={
                styles.channelMsg
                + (isContinuation ? ` ${styles.channelMsgContinuation}` : '')
                + (isSelf ? ` ${styles.channelMsgSelf}` : '')
              }
            >
              <div className={styles.channelMsgAvatar}>
                <MemberAvatar info={senderInfo} className={styles.channelMsgAvatarImg} />
              </div>
              <div className={styles.channelMsgBody}>
                {!isContinuation && (
                  <div className={styles.channelMsgHeader}>
                    <span className={styles.channelMsgSender}>{senderInfo.displayName}</span>
                    <span className={styles.channelMsgTime}>{formatChannelTime(msg.timestamp)}</span>
                  </div>
                )}
                <MarkdownContent
                  className={styles.channelMsgText}
                  html={renderMarkdown(msg.body || '')}
                />
              </div>
            </div>
          );
          lastSender = msg.sender;
          return el;
        })}
      </div>
      {showNewMessages && (
        <button
          type="button"
          className={styles.channelNewMessagesBtn}
          onClick={scrollToBottom}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M12 5v14" />
            <path d="m19 12-7 7-7-7" />
          </svg>
          <span>{t('channel.newMessages')}</span>
        </button>
      )}
    </>
  );
}

// ── ChannelMembers — 右侧面板成员列表

function MemberItem({ info, memberId, onRemove, removeDisabled, removeTitle }: {
  info: MemberInfo;
  memberId?: string;
  onRemove?: (memberId: string, info: MemberInfo) => void;
  removeDisabled?: boolean;
  removeTitle?: string;
}) {
  return (
    <div className={styles.channelMemberItem}>
      <div className={styles.channelMemberAvatar}>
        <MemberAvatar info={info} className={styles.channelMemberAvatarImg} />
      </div>
      <div className={styles.channelMemberName}>{info.displayName}</div>
      {onRemove && (
        <button
          type="button"
          className={styles.channelMemberRemoveButton}
          title={removeTitle}
          disabled={removeDisabled}
          onClick={() => onRemove(memberId || info.id, info)}
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
            <path d="M18 6 6 18" />
            <path d="m6 6 12 12" />
          </svg>
        </button>
      )}
    </div>
  );
}

export function ChannelMembers() {
  const { t } = useI18n();
  const currentChannel = useStore(s => s.currentChannel);
  const channels = useStore(s => s.channels);
  const channelMembers = useStore(s => s.channelMembers);
  const isDM = useStore(s => s.channelIsDM);
  const agents = useStore(s => s.agents);
  const userName = useStore(s => s.userName);
  const userAvatarUrl = useStore(s => s.userAvatarUrl);
  const currentAgentId = useStore(s => s.currentAgentId);
  const [menuPos, setMenuPos] = useState<{ x: number; y: number } | null>(null);
  const [busyMemberId, setBusyMemberId] = useState<string | null>(null);
  const connectionKey = useStore(s => JSON.stringify(resolveServerConnection(s)));
  const operationVersion = useRef(0);
  useEffect(() => {
    operationVersion.current += 1;
    setBusyMemberId(null);
    setMenuPos(null);
  }, [currentChannel, currentAgentId, connectionKey]);

  const agentMap = useMemo(() => buildAgentMap(agents), [agents]);

  if (!currentChannel) return null;

  const ch = channels.find((channel) => channel.id === currentChannel);
  const dmOwnerId = resolveDmOwnerId(ch, currentAgentId);
  const resolve = (id: string) => resolveChannelMember(id, userName, userAvatarUrl, agents, isDM ? dmOwnerId : currentAgentId, agentMap);

  if (isDM) {
    const peerInfo = resolve(channelMembers[0] || '');
    const selfInfo = resolve(dmOwnerId);
    return <>{[peerInfo, selfInfo].map(i => <MemberItem key={i.id} info={i} />)}</>;
  }

  const availableAgents = agents.filter((agent) => !channelMembers.includes(agent.id));
  const canRemoveMembers = channelMembers.length > 2;

  const handleAddClick = (event: React.MouseEvent<HTMLButtonElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    setMenuPos({ x: rect.left, y: rect.bottom + 4 });
  };

  const handleRemove = (memberId: string, info: MemberInfo) => {
    if (!currentChannel || !canRemoveMembers) return;
    const confirmed = confirm(t('channel.removeMemberConfirm', { name: info.displayName }));
    if (!confirmed) return;
    const channelId = currentChannel;
    const scope = captureConversationRequestScope(true);
    const version = operationVersion.current;
    const isCurrent = () => scope.isCurrent() && version === operationVersion.current;
    setBusyMemberId(memberId);
    removeChannelMember(channelId, memberId)
      .then(() => { if (isCurrent()) requestChannelComposerFocus(channelId); })
      .catch((err) => { if (isCurrent()) alert(err?.message || t('channel.removeMemberFailed')); })
      .finally(() => { if (isCurrent()) setBusyMemberId(null); scope.dispose(); });
  };

  const userInfo = resolve('user');
  const menuItems: ContextMenuItem[] = availableAgents.length > 0
    ? availableAgents.map((agent): ContextMenuItem => ({
      label: agent.name || agent.id,
      action: () => {
        if (!currentChannel) return;
        const scope = captureConversationRequestScope(true);
        const version = operationVersion.current;
        const isCurrent = () => scope.isCurrent() && version === operationVersion.current;
        setBusyMemberId(agent.id);
        addChannelMember(currentChannel, agent.id)
          .catch((err) => { if (isCurrent()) alert(err?.message || t('channel.addMemberFailed')); })
          .finally(() => { if (isCurrent()) setBusyMemberId(null); scope.dispose(); });
      },
    }))
    : [{ label: t('channel.noAvailableMembers'), disabled: true }];

  return (
    <>
      <MemberItem info={userInfo} />
      {channelMembers.map((memberId) => (
        <MemberItem
          key={memberId}
          memberId={memberId}
          info={resolve(memberId)}
          onRemove={handleRemove}
          removeDisabled={!canRemoveMembers || busyMemberId === memberId}
          removeTitle={canRemoveMembers ? t('channel.removeMember') : t('channel.minMembers')}
        />
      ))}
      <button
        type="button"
        className={styles.channelMemberAddButton}
        onClick={handleAddClick}
        disabled={!!busyMemberId}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
          <path d="M12 5v14" />
          <path d="M5 12h14" />
        </svg>
        <span>{t('channel.addMember')}</span>
      </button>
      {menuPos && (
        <ContextMenu
          items={menuItems}
          position={menuPos}
          onClose={() => setMenuPos(null)}
        />
      )}
    </>
  );
}

function activitySessionPath(activity: AgentPhoneActivity | undefined): string | null {
  const value = activity?.details?.sessionPath;
  return typeof value === 'string' && value ? value : null;
}

function channelTickerText(status: ChannelTickerStatus | null | undefined, t: (key: string, vars?: Record<string, string | number>) => string): string | null {
  if (!status) return null;
  const active = status.active;
  if (active) {
    return t('channel.deliveryActive', {
      agent: active.agentId || active.activeAgentId || '',
      done: active.delivered ?? 0,
      total: active.agentCount ?? 0,
    });
  }
  const dueAt = status.nextReminder?.dueAtMs || (status.nextReminder?.dueAt ? Date.parse(status.nextReminder.dueAt) : 0);
  if (dueAt && Number.isFinite(dueAt)) {
    const time = new Date(dueAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    return t('channel.nextReminder', { time });
  }
  if (status.queued) return t('channel.deliveryQueued');
  return null;
}

function createPhoneStreamMessage(agentId: string, turnToken: number): ChatMessage {
  return {
    id: `${PHONE_STREAM_MESSAGE_PREFIX}-${agentId}-${turnToken}`,
    role: 'assistant',
    blocks: [],
  };
}

function upsertPhoneBlock(
  blocks: ContentBlock[],
  match: (block: ContentBlock) => boolean,
  nextBlock: ContentBlock,
  insertAtStart = false,
): ContentBlock[] {
  const idx = blocks.findIndex(match);
  if (idx >= 0) {
    const next = [...blocks];
    next[idx] = nextBlock;
    return next;
  }
  return insertAtStart ? [nextBlock, ...blocks] : [...blocks, nextBlock];
}

export function AgentPhoneSessionPreview({ sessionPath, agentId, agentYuan }: {
  sessionPath: string | null;
  agentId: string;
  agentYuan?: string | null;
}) {
  const { t } = useI18n();
  const connectionKey = useStore(s => JSON.stringify(resolveServerConnection(s)));
  const currentAgentId = useStore(s => s.currentAgentId);
  const session = useStore(s => (sessionPath ? sessionScopedValue(s, s.chatSessions, sessionPath) ?? null : null));
  const items = session?.items ?? EMPTY_CHAT_ITEMS;
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [retryVersion, setRetryVersion] = useState(0);
  const [streamMessage, setStreamMessage] = useState<ChatMessage | null>(null);
  const [streamRevision, setStreamRevision] = useState(0);
  const streamTurnRef = useRef(0);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const bottomScroll = useContinuousBottomScroll({
    scrollRef,
    contentRef,
    active: !!sessionPath,
    stickyThreshold: 32,
  });
  const moodYuan = agentYuan || 'lingxi';

  // Switch landing runs in the layout phase (before paint) so the panel never shows a wrong
  // scrollTop frame. Only arm an instant landing when the target session has no messages yet:
  // the first async hydrate (0 -> N) then snaps without animating. If the session is already
  // loaded, the instant scroll below lands it and later growth = streaming (keeps smooth follow).
  useLayoutEffect(() => {
    const alreadyHydrated = !!sessionPath
      && ((sessionScopedValue(useStore.getState(), useStore.getState().chatSessions, sessionPath)?.items?.length ?? 0) > 0);
    if (sessionPath && !alreadyHydrated) bottomScroll.armInstantLanding();
    bottomScroll.scrollToBottom({ mode: 'instant', forceSticky: true });
    streamTurnRef.current = 0;
    setStreamMessage(null);
    setStreamRevision(0);
  }, [bottomScroll, sessionPath, connectionKey, currentAgentId]);

  useEffect(() => {
    setLoadError(false);
    if (!sessionPath || items.length > 0) { setLoading(false); return; }
    let cancelled = false;
    const scope = captureConversationRequestScope();
    setLoading(true);
    void loadMessages(sessionPath)
      .catch((error: unknown) => {
        if (!cancelled && scope.isCurrent()) {
          console.warn('[channels] load phone session failed:', error);
          setLoadError(true);
        }
      })
      .finally(() => {
        if (!cancelled && scope.isCurrent()) setLoading(false);
        scope.dispose();
      });
    return () => { cancelled = true; scope.dispose(); };
  }, [items.length, sessionPath, retryVersion, connectionKey, currentAgentId]);

  useEffect(() => {
    bottomScroll.followBottom();
  }, [bottomScroll, items.length, loading, streamRevision]);

  useEffect(() => {
    if (!sessionPath) return;

    const scope = captureConversationRequestScope(true);
    const updateStreamMessage = (updater: (message: ChatMessage) => ChatMessage) => {
      if (!scope.isCurrent()) return;
      setStreamMessage((prev) => {
        const base = prev || createPhoneStreamMessage(agentId, ++streamTurnRef.current);
        return updater(base);
      });
      setStreamRevision((value) => value + 1);
    };

    const unsubscribe = subscribeStreamKey(sessionPath, (event: any) => {
      if (!scope.isCurrent()) return;
      switch (event.type) {
        case 'thinking_start':
          updateStreamMessage((message) => ({
            ...message,
            blocks: upsertPhoneBlock(
              message.blocks || [],
              (block) => block.type === 'thinking',
              { type: 'thinking', content: '', sealed: false },
              true,
            ),
          }));
          break;
        case 'thinking_delta':
          updateStreamMessage((message) => {
            const blocks = message.blocks || [];
            const thinking = blocks.find((block) => block.type === 'thinking') as Extract<ContentBlock, { type: 'thinking' }> | undefined;
            return {
              ...message,
              blocks: upsertPhoneBlock(
                blocks,
                (block) => block.type === 'thinking',
                { type: 'thinking', content: `${thinking?.content || ''}${event.delta || ''}`, sealed: false },
                true,
              ),
            };
          });
          break;
        case 'thinking_end':
          updateStreamMessage((message) => {
            const blocks = message.blocks || [];
            const thinking = blocks.find((block) => block.type === 'thinking') as Extract<ContentBlock, { type: 'thinking' }> | undefined;
            return {
              ...message,
              blocks: upsertPhoneBlock(
                blocks,
                (block) => block.type === 'thinking',
                { type: 'thinking', content: thinking?.content || '', sealed: true },
                true,
              ),
            };
          });
          break;
        case 'mood_start':
          updateStreamMessage((message) => ({
            ...message,
            blocks: upsertPhoneBlock(
              message.blocks || [],
              (block) => block.type === 'mood',
              { type: 'mood', yuan: moodYuan, text: '' },
            ),
          }));
          break;
        case 'mood_text':
          updateStreamMessage((message) => {
            const blocks = message.blocks || [];
            const mood = blocks.find((block) => block.type === 'mood') as Extract<ContentBlock, { type: 'mood' }> | undefined;
            return {
              ...message,
              blocks: upsertPhoneBlock(
                blocks,
                (block) => block.type === 'mood',
                { type: 'mood', yuan: moodYuan, text: `${mood?.text || ''}${event.delta || ''}` },
              ),
            };
          });
          break;
        case 'mood_end':
          break;
        case 'text_delta':
          updateStreamMessage((message) => {
            const blocks = message.blocks || [];
            const textBlock = blocks.find((block) => block.type === 'text') as (Extract<ContentBlock, { type: 'text' }> & { _raw?: string }) | undefined;
            const prevText = textBlock?.source ?? textBlock?._raw ?? '';
            const nextText = `${prevText}${event.delta || ''}`;
            return {
              ...message,
              blocks: upsertPhoneBlock(
                blocks,
                (block) => block.type === 'text',
                { type: 'text', source: nextText },
              ),
            };
          });
          break;
        case 'tool_start':
          updateStreamMessage((message) => ({
            ...message,
            blocks: [
              ...(message.blocks || []),
              {
                type: 'tool_group',
                tools: [toolCallFromStartEvent(event)],
                collapsed: false,
              },
            ],
          }));
          break;
        case 'tool_end':
          updateStreamMessage((message) => {
            const blocks = [...(message.blocks || [])];
            for (let i = blocks.length - 1; i >= 0; i -= 1) {
              const block = blocks[i];
              if (block.type !== 'tool_group') continue;
              const toolIndex = findOpenToolIndex(block.tools, event);
              if (toolIndex < 0) continue;
              const tools = [...block.tools];
              const id = toolCallIdFromEvent(event);
              tools[toolIndex] = {
                ...tools[toolIndex],
                ...(id ? { id } : {}),
                done: true,
                success: !!event.success,
                details: event.details,
              };
              blocks[i] = { ...block, tools, collapsed: tools.length > 1 && tools.every((tool) => tool.done) };
              break;
            }
            return { ...message, blocks };
          });
          break;
        case 'content_block':
          if (event.block) {
            updateStreamMessage((message) => ({
              ...message,
              blocks: [...(message.blocks || []), event.block],
            }));
          }
          break;
        case 'turn_end':
          void loadMessages(sessionPath).then(() => { if (scope.isCurrent()) setStreamMessage(null); })
            .catch(() => { if (scope.isCurrent()) setLoadError(true); });
          break;
        default:
          break;
      }
    });

    return () => { unsubscribe(); scope.dispose(); };
  }, [agentId, moodYuan, sessionPath, connectionKey, currentAgentId]);

  const phoneNotice = t('channel.phoneReceivedNewMessages') || '收到新的群聊消息';
  const displayItems = useMemo(() => {
    const mergedItems = streamMessage
      ? [...items, { type: 'message' as const, data: streamMessage }]
      : items;
    return mergedItems.map((item) => {
      if (item.type !== 'message' || item.data.role !== 'user') return item;
      return {
        ...item,
        data: {
          ...item.data,
          text: phoneNotice,
          textHtml: renderMarkdown(phoneNotice),
          attachments: [],
          deskContext: null,
          skills: [],
        },
      };
    });
  }, [items, phoneNotice, streamMessage]);

  return (
    <div ref={scrollRef} className={`${styles.agentActivityTranscriptScroll} ${chatStyles.subagentPreviewTranscript}`}>
      <div ref={contentRef}>
        {!sessionPath ? (
          <div className={styles.agentActivityEmpty}>{t('channel.agentIdle')}</div>
        ) : loadError && displayItems.length === 0 ? (
          <div role="alert" className={styles.agentActivityEmpty}>
            <span>{t('channel.loadFailed')}</span>
            <button type="button" onClick={() => setRetryVersion(value => value + 1)}>{t('channel.retry')}</button>
          </div>
        ) : loading && displayItems.length === 0 ? (
          <div className={styles.agentActivityEmpty}>{t('common.loading')}</div>
        ) : displayItems.length === 0 ? (
          <div className={styles.agentActivityEmpty}>{t('channel.agentIdle')}</div>
        ) : (
          <ChatTranscript
            items={displayItems}
            sessionPath={sessionPath}
            agentId={agentId}
            readOnly
            hideUserIdentity
          />
        )}
      </div>
    </div>
  );
}

// ── ChannelAgentActivityPanel — 右侧面板 Agent 动态

export function ChannelAgentActivityPanel() {
  const { t } = useI18n();
  const currentChannel = useStore(s => s.currentChannel);
  const channels = useStore(s => s.channels);
  const channelMembers = useStore(s => s.channelMembers);
  const isDM = useStore(s => s.channelIsDM);
  const agents = useStore(s => s.agents);
  const userName = useStore(s => s.userName);
  const userAvatarUrl = useStore(s => s.userAvatarUrl);
  const currentAgentId = useStore(s => s.currentAgentId);
  const allActivities = useStore(s => s.channelAgentActivities);
  const tickerStatuses = useStore(s => s.channelTickerStatus);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  const agentMap = useMemo(() => buildAgentMap(agents), [agents]);

  if (!currentChannel) return null;

  const ch = channels.find((channel) => channel.id === currentChannel);
  const dmOwnerId = resolveDmOwnerId(ch, currentAgentId);
  const ids = isDM
    ? [dmOwnerId, channelMembers[0]].filter((id): id is string => !!id)
    : channelMembers;
  const uniqueIds = Array.from(new Set(ids));
  if (uniqueIds.length === 0) return null;

  const byAgent = allActivities?.[currentChannel] || {};
  const tickerText = channelTickerText(tickerStatuses?.[currentChannel], t);
  const resolve = (id: string) => resolveChannelMember(id, userName, userAvatarUrl, agents, isDM ? dmOwnerId : currentAgentId, agentMap);

  return (
    <div className="universal-card">
      <div className="channel-info-section">
        <div className={styles.agentActivityHeader}>
          <div className="channel-info-label">{t('channel.agentActivity')}</div>
        </div>
        {tickerText && <div className={styles.agentActivityScheduler}>{tickerText}</div>}
        <div className={styles.agentActivityList}>
          {uniqueIds.map((agentId) => {
            const info = resolve(agentId);
            const history = byAgent[agentId] || [];
            const latest = history[0];
            const open = expanded[agentId] === true;
            const sessionPath = history.map(activitySessionPath).find((path): path is string => !!path) || null;
            return (
              <div key={agentId} className={styles.agentActivityItem}>
                <button
                  type="button"
                  className={styles.agentActivityRow}
                  onClick={() => setExpanded(prev => ({ ...prev, [agentId]: !prev[agentId] }))}
                  aria-expanded={open}
                >
                  <span className={styles.agentActivityAvatar}>
                    <MemberAvatar info={info} className={styles.agentActivityAvatarImg} />
                  </span>
                  <span className={styles.agentActivityName}>{info.displayName}</span>
                  <span className={styles.agentActivitySummary}>{latest?.summary || t('channel.agentIdle')}</span>
                </button>
                {open && (
                  <div className={styles.agentActivityDetails}>
                    <AgentPhoneSessionPreview
                      sessionPath={sessionPath}
                      agentId={agentId}
                      agentYuan={info.yuan}
                    />
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

export function ChannelExportPanel() {
  const { t } = useI18n();
  const addToast = useStore(s => s.addToast);
  const [exporting, setExporting] = useState(false);

  const handleExport = useCallback(async () => {
    if (exporting) return;
    setExporting(true);
    const scope = captureConversationRequestScope(true);
    try {
      const filename = await exportCurrentConversation();
      if (scope.isCurrent()) addToast(t('channel.exportSuccess', { filename }), 'success');
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (scope.isCurrent()) addToast(t('channel.exportFailed', { message }), 'error');
    } finally {
      setExporting(false);
      scope.dispose();
    }
  }, [addToast, exporting, t]);

  return (
    <div className="universal-card">
      <div className="channel-info-section">
        <div className="channel-info-label">{t('channel.exportTitle')}</div>
        <div className={styles.channelExportDescription}>{t('channel.exportDescription')}</div>
        <Button
          className={styles.channelExportButton}
          size="md"
          loading={exporting}
          onClick={() => { void handleExport(); }}
        >
          {exporting ? t('channel.exporting') : t('channel.exportAction')}
        </Button>
      </div>
    </div>
  );
}

function parseOptionalIntInput(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const num = Number(trimmed);
  if (!Number.isFinite(num) || num <= 0) return null;
  return Math.floor(num);
}

function modelKey(model: { id: string; provider: string } | null | undefined): string {
  return model ? `${model.provider}/${model.id}` : '';
}

export function ChannelAgentSettingsPanel() {
  const { t } = useI18n();
  const currentChannel = useStore(s => s.currentChannel);
  const isDM = useStore(s => s.channelIsDM);
  const models = useStore(s => s.models);
  const connectionKey = useStore(s => JSON.stringify(resolveServerConnection(s)));
  const currentAgentId = useStore(s => s.currentAgentId);
  const settingsError = useStore(s => s.channelSettingsError);
  const settingsLoading = useStore(s => s.channelSettingsLoading);
  const toolMode = useStore(s => s.channelAgentPhoneToolMode);
  const replyMinChars = useStore(s => s.channelAgentReplyMinChars);
  const replyMaxChars = useStore(s => s.channelAgentReplyMaxChars);
  const proactiveEnabled = useStore(s => s.channelAgentProactiveEnabled);
  const reminderIntervalMinutes = useStore(s => s.channelAgentReminderIntervalMinutes);
  const guardLimit = useStore(s => s.channelAgentGuardLimit);
  const modelOverrideEnabled = useStore(s => s.channelAgentModelOverrideEnabled);
  const modelOverrideModel = useStore(s => s.channelAgentModelOverrideModel);
  const [saving, setSaving] = useState(false);
  const [modelOpen, setModelOpen] = useState(false);
  const [draftMin, setDraftMin] = useState(replyMinChars ? String(replyMinChars) : '');
  const [draftMax, setDraftMax] = useState(replyMaxChars ? String(replyMaxChars) : '');
  const [draftReminder, setDraftReminder] = useState(String(reminderIntervalMinutes || 31));
  const [draftGuardLimit, setDraftGuardLimit] = useState(String(guardLimit || 36));
  const modelSelectRef = useRef<HTMLDivElement | null>(null);
  const saveVersion = useRef(0);
  useEffect(() => {
    saveVersion.current += 1;
    setSaving(false);
    setModelOpen(false);
  }, [currentChannel, currentAgentId, connectionKey]);

  useEffect(() => {
    setDraftMin(replyMinChars ? String(replyMinChars) : '');
    setDraftMax(replyMaxChars ? String(replyMaxChars) : '');
    setDraftReminder(String(reminderIntervalMinutes || 31));
    setDraftGuardLimit(String(guardLimit || 36));
  }, [currentChannel, replyMinChars, replyMaxChars, reminderIntervalMinutes, guardLimit]);

  useEffect(() => {
    if (models.length > 0) return;
    const scope = captureConversationRequestScope();
    void lingxiFetch('/api/models')
      .then(res => res.ok ? res.json() : null)
      .then(data => {
        if (scope.isCurrent() && Array.isArray(data?.models)) useStore.setState({ models: data.models });
      })
      .catch((err: unknown) => console.warn('[channels] load models failed:', err));
    return scope.dispose;
  }, [models.length, connectionKey, currentAgentId]);

  useEffect(() => {
    if (!modelOpen) return;
    const handler = (event: MouseEvent) => {
      if (modelSelectRef.current && !modelSelectRef.current.contains(event.target as Node)) {
        setModelOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [modelOpen]);

  if (!currentChannel) return null;
  if (settingsError || settingsLoading) return (
    <div className="universal-card" role={settingsError ? 'alert' : 'status'}>
      <span>{t(settingsError ? 'channel.loadFailed' : 'common.loading')}</span>
      {settingsError && <button type="button" onClick={() => { void loadConversationAgentPhoneSettings(currentChannel); }}>{t('channel.retry')}</button>}
    </div>
  );

  const saveSettings = async (patch: Parameters<typeof saveConversationAgentPhoneSettings>[0]) => {
    setSaving(true);
    const scope = captureConversationRequestScope(true);
    const version = saveVersion.current;
    const isCurrent = () => scope.isCurrent() && version === saveVersion.current;
    try {
      await saveConversationAgentPhoneSettings(patch);
    } catch (err: any) {
      console.error('[channels] save phone settings failed:', err);
      if (isCurrent()) alert(err?.message || t('channel.settingsSaveFailed'));
    } finally {
      if (isCurrent()) setSaving(false);
      scope.dispose();
    }
  };

  const commitTextSettings = () => {
    const min = parseOptionalIntInput(draftMin);
    const max = parseOptionalIntInput(draftMax);
    const reminder = parseOptionalIntInput(draftReminder) || 31;
    const guard = parseOptionalIntInput(draftGuardLimit) || 36;
    if (min && max && min > max) {
      alert(t('channel.replyRangeInvalid'));
      return;
    }
    void saveSettings({
      replyMinChars: min,
      replyMaxChars: max,
      ...(!isDM ? { reminderIntervalMinutes: reminder, guardLimit: guard } : {}),
    });
  };

  const changeMode = (mode: 'read_only' | 'write') => {
    if (mode === toolMode || saving) return;
    void saveSettings({ mode });
  };

  const changeProactiveEnabled = (enabled: boolean) => {
    if (saving || enabled === proactiveEnabled) return;
    void saveSettings({ proactiveEnabled: enabled });
  };

  const changeModelOverrideEnabled = (enabled: boolean) => {
    if (saving || enabled === modelOverrideEnabled) return;
    if (!enabled) {
      void saveSettings({ modelOverrideEnabled: false, modelOverrideModel: modelOverrideModel });
      return;
    }
    const selected = modelOverrideModel || models[0] || null;
    if (!selected) {
      alert(t('channel.noModelsAvailable'));
      return;
    }
    void saveSettings({
      modelOverrideEnabled: true,
      modelOverrideModel: { id: selected.id, provider: selected.provider },
    });
  };

  const selectOverrideModel = (model: Model) => {
    setModelOpen(false);
    void saveSettings({
      modelOverrideEnabled: true,
      modelOverrideModel: { id: model.id, provider: model.provider },
    });
  };

  const selectedModel = modelOverrideModel
    ? models.find(m => m.id === modelOverrideModel.id && m.provider === modelOverrideModel.provider)
    : null;
  const modelLabel = selectedModel?.name || modelOverrideModel?.id || t('channel.selectModel');
  const groupedModels = models.reduce<Record<string, Model[]>>((acc, model) => {
    const key = model.provider || '';
    if (!acc[key]) acc[key] = [];
    acc[key].push(model);
    return acc;
  }, {});
  const providerKeys = Object.keys(groupedModels);
  const hasMultipleProviders = providerKeys.length > 1 || (providerKeys.length === 1 && providerKeys[0] !== '');

  return (
    <div className={`universal-card ${styles.agentSettingsCard}`}>
      <div className="channel-info-section">
        <div className={styles.agentSettingsHeader}>
          <div className="channel-info-label">{t('channel.agentSettings')}</div>
        </div>
        <div className={!isDM ? styles.agentSettingsInlineGrid : undefined}>
          <div className={styles.agentSettingsField}>
            <div className={styles.agentSettingsLabel}>{t('channel.toolPermission')}</div>
            <div className={`${styles.agentToolModeToggle}${!isDM ? ` ${styles.agentToolModeToggleFill}` : ''}`}>
              <button
                type="button"
                className={`${styles.agentToolModeButton}${toolMode === 'read_only' ? ` ${styles.agentToolModeButtonActive}` : ''}`}
                disabled={saving}
                onClick={() => changeMode('read_only')}
              >
                {t('channel.toolReadOnly')}
              </button>
              <button
                type="button"
                className={`${styles.agentToolModeButton}${toolMode === 'write' ? ` ${styles.agentToolModeButtonActive}` : ''}`}
                disabled={saving}
                onClick={() => changeMode('write')}
              >
                {t('channel.toolWrite')}
              </button>
            </div>
          </div>
          {!isDM && (
            <div className={styles.agentSettingsField}>
              <div className={styles.agentSettingsLabel}>{t('channel.guardLimit')}</div>
              <input
                className={styles.agentReplyRangeInput}
                inputMode="numeric"
                placeholder="36"
                value={draftGuardLimit}
                onChange={(event) => setDraftGuardLimit(event.target.value.replace(/[^\d]/g, ''))}
                onBlur={commitTextSettings}
                disabled={saving}
              />
            </div>
          )}
        </div>
        {!isDM && (
          <div className={`${styles.agentSettingsInlineGrid} ${styles.agentSettingsInlineGridSpaced}`}>
            <div className={styles.agentSettingsField}>
              <div className={styles.agentSettingsLabel}>{t('channel.proactiveInitiation')}</div>
              <div className={`${styles.agentToolModeToggle} ${styles.agentToolModeToggleFill}`}>
                <button
                  type="button"
                  className={`${styles.agentToolModeButton}${!proactiveEnabled ? ` ${styles.agentToolModeButtonActive}` : ''}`}
                  disabled={saving}
                  onClick={() => changeProactiveEnabled(false)}
                >
                  {t('channel.proactiveOff')}
                </button>
                <button
                  type="button"
                  className={`${styles.agentToolModeButton}${proactiveEnabled ? ` ${styles.agentToolModeButtonActive}` : ''}`}
                  disabled={saving}
                  onClick={() => changeProactiveEnabled(true)}
                >
                  {t('channel.proactiveOn')}
                </button>
              </div>
            </div>
            <div className={styles.agentSettingsField}>
              <div className={styles.agentSettingsLabel}>{t('channel.proactiveInterval')}</div>
              <input
                className={styles.agentReplyRangeInput}
                inputMode="numeric"
                placeholder="31"
                value={draftReminder}
                onChange={(event) => setDraftReminder(event.target.value.replace(/[^\d]/g, ''))}
                onBlur={commitTextSettings}
                disabled={saving || !proactiveEnabled}
              />
            </div>
          </div>
        )}
        <div className={styles.agentSettingsField}>
          <div className={styles.agentSettingsLabel}>{t('channel.replyRange')}</div>
          <div className={styles.agentReplyRangeRow}>
            <input
              className={styles.agentReplyRangeInput}
              inputMode="numeric"
              placeholder={t('channel.replyMinPlaceholder')}
              value={draftMin}
              onChange={(event) => setDraftMin(event.target.value.replace(/[^\d]/g, ''))}
              onBlur={commitTextSettings}
              disabled={saving}
            />
            <span className={styles.agentReplyRangeSep}>-</span>
            <input
              className={styles.agentReplyRangeInput}
              inputMode="numeric"
              placeholder={t('channel.replyMaxPlaceholder')}
              value={draftMax}
              onChange={(event) => setDraftMax(event.target.value.replace(/[^\d]/g, ''))}
              onBlur={commitTextSettings}
              disabled={saving}
            />
          </div>
        </div>
        {!isDM && (
          <div className={styles.agentSettingsField}>
            <div className={styles.agentSettingsLabel}>{t('channel.modelOverride')}</div>
            <div className={styles.agentModelOverrideRow}>
              <div className={styles.agentToolModeToggle}>
                <button
                  type="button"
                  className={`${styles.agentToolModeButton}${!modelOverrideEnabled ? ` ${styles.agentToolModeButtonActive}` : ''}`}
                  disabled={saving}
                  onClick={() => changeModelOverrideEnabled(false)}
                >
                  {t('channel.modelDefault')}
                </button>
                <button
                  type="button"
                  className={`${styles.agentToolModeButton}${modelOverrideEnabled ? ` ${styles.agentToolModeButtonActive}` : ''}`}
                  disabled={saving}
                  onClick={() => changeModelOverrideEnabled(true)}
                >
                  {t('channel.modelOverrideOn')}
                </button>
              </div>
              <div className={styles.agentModelSelect} ref={modelSelectRef}>
                <button
                  type="button"
                  className={styles.agentModelSelectButton}
                  disabled={!modelOverrideEnabled || saving}
                  onClick={() => setModelOpen(open => !open)}
                >
                  <span>{modelLabel}</span>
                  <span className={styles.agentModelSelectArrow}>▾</span>
                </button>
                {modelOpen && modelOverrideEnabled && (
                  <div className={styles.agentModelDropdown}>
                    {providerKeys.map(provider => (
                      <div key={provider || '__none'}>
                        {hasMultipleProviders && (
                          <div className={styles.agentModelGroupHeader}>{provider || '-'}</div>
                        )}
                        {groupedModels[provider].map(model => (
                          <button
                            key={modelKey(model)}
                            type="button"
                            className={`${styles.agentModelOption}${modelKey(modelOverrideModel) === modelKey(model) ? ` ${styles.agentModelOptionActive}` : ''}`}
                            onClick={() => selectOverrideModel(model)}
                          >
                            {model.name || model.id}
                          </button>
                        ))}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ── ChannelInput — 输入区域 + @mention

export function ChannelInput() {
  const currentChannel = useStore(s => s.currentChannel);
  const isDM = useStore(s => s.channelIsDM);
  const channelMembers = useStore(s => s.channelMembers);
  const agents = useStore(s => s.agents);
  const userName = useStore(s => s.userName);
  const userAvatarUrl = useStore(s => s.userAvatarUrl);
  const currentAgentId = useStore(s => s.currentAgentId);

  const agentMap = useMemo(() => buildAgentMap(agents), [agents]);
  const [inputValue, setInputValue] = useState('');
  const [sending, setSending] = useState(false);
  const [mentionActive, setMentionActive] = useState(false);
  const [mentionItems, setMentionItems] = useState<MemberInfo[]>([]);
  const [mentionSelectedIdx, setMentionSelectedIdx] = useState(0);
  const [mentionStartPos, setMentionStartPos] = useState(-1);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const composerVersion = useRef(0);
  const connectionKey = useStore(s => JSON.stringify(resolveServerConnection(s)));
  useEffect(() => {
    composerVersion.current += 1;
    setInputValue('');
    setSending(false);
    setMentionActive(false);
  }, [currentChannel, currentAgentId, connectionKey]);

  const handleSend = useCallback(async () => {
    if (sending || isDM || !inputValue.trim()) return;
    const version = composerVersion.current;
    const scope = captureConversationRequestScope(true);
    const isCurrent = () => scope.isCurrent() && composerVersion.current === version;
    setSending(true);
    try {
      await sendChannelMessage(inputValue.trim());
      if (isCurrent()) setInputValue('');
    } catch {
      if (isCurrent()) useStore.getState().addToast(window.t('channel.sendFailed'), 'error');
    } finally {
      if (isCurrent()) setSending(false);
      scope.dispose();
    }
  }, [sending, isDM, inputValue]);

  const checkMention = useCallback(() => {
    if (!inputRef.current) return;
    const val = inputRef.current.value;
    const pos = inputRef.current.selectionStart ?? 0;
    const before = val.slice(0, pos);
    const atIdx = before.lastIndexOf('@');
    if (atIdx < 0 || (atIdx > 0 && /\S/.test(before[atIdx - 1]))) { setMentionActive(false); return; }
    const keyword = before.slice(atIdx + 1).toLowerCase();
    setMentionStartPos(atIdx);
    const members = (channelMembers || [])
      .map(id => resolveChannelMember(id, userName, userAvatarUrl, agents, currentAgentId, agentMap))
      .filter(m => !m.isUser);
    const filtered = keyword
      ? members.filter(m => m.displayName.toLowerCase().includes(keyword) || (m.yuan || '').toLowerCase().includes(keyword))
      : members;
    if (filtered.length === 0) { setMentionActive(false); return; }
    setMentionItems(filtered);
    setMentionSelectedIdx(0);
    setMentionActive(true);
  }, [channelMembers, agents, agentMap, userName, userAvatarUrl, currentAgentId]);

  const insertMention = useCallback((name: string) => {
    if (!inputRef.current || mentionStartPos < 0) return;
    const val = inputRef.current.value;
    const pos = inputRef.current.selectionStart ?? 0;
    const before = val.slice(0, mentionStartPos);
    const inserted = `@${name} `;
    setInputValue(before + inserted + val.slice(pos));
    setMentionActive(false);
    requestAnimationFrame(() => {
      if (!inputRef.current) return;
      const c = before.length + inserted.length;
      inputRef.current.setSelectionRange(c, c);
      inputRef.current.focus();
    });
  }, [mentionStartPos]);

  useEffect(() => {
    const handleComposerFocus = (event: Event) => {
      const detail = (event as CustomEvent<{ channelId?: string }>).detail;
      if (!currentChannel || detail?.channelId !== currentChannel) return;
      requestAnimationFrame(() => {
        const input = inputRef.current;
        if (!input) return;
        const pos = input.value.length;
        input.focus();
        input.setSelectionRange(pos, pos);
      });
    };
    window.addEventListener(CHANNEL_COMPOSER_FOCUS_EVENT, handleComposerFocus);
    return () => window.removeEventListener(CHANNEL_COMPOSER_FOCUS_EVENT, handleComposerFocus);
  }, [currentChannel]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !(e.nativeEvent as any).isComposing) {
      e.preventDefault();
      if (mentionActive) { const s = mentionItems[mentionSelectedIdx]; if (s) insertMention(s.displayName); }
      else handleSend();
      return;
    }
    if (!mentionActive) return;
    if (e.key === 'ArrowDown') { e.preventDefault(); setMentionSelectedIdx(i => (i + 1) % mentionItems.length); }
    if (e.key === 'ArrowUp') { e.preventDefault(); setMentionSelectedIdx(i => (i - 1 + mentionItems.length) % mentionItems.length); }
    if (e.key === 'Escape') { e.preventDefault(); setMentionActive(false); }
  }, [mentionActive, mentionItems, mentionSelectedIdx, insertMention, handleSend]);

  const handleInput = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setInputValue(e.target.value);
    requestAnimationFrame(() => checkMention());
  }, [checkMention]);

  if (isDM || !currentChannel) return null;

  return (
    <div className={styles.channelInputWrapper}>
      {mentionActive && mentionItems.length > 0 && (
        <div className={styles.channelMentionDropdown}>
          {mentionItems.map((m) => (
            <div
              key={m.id}
              className={`${styles.channelMentionItem}${mentionItems.indexOf(m) === mentionSelectedIdx ? ` ${styles.channelMentionItemActive}` : ''}`}
              data-name={m.displayName}
              onMouseDown={(e) => {
                e.preventDefault();
                insertMention(m.displayName);
              }}
            >
              <div className={styles.channelMentionAvatar}>
                <MemberAvatar info={m} />
              </div>
              <span>{m.displayName}</span>
            </div>
          ))}
        </div>
      )}
      <textarea
        ref={inputRef}
        className={styles.channelInputBox}
        placeholder={window.t?.('channel.inputPlaceholder') || 'Send a message...'}
        rows={1}
        spellCheck={false}
        value={inputValue}
        onChange={handleInput}
        onKeyDown={handleKeyDown}
      />
      <button
        className={styles.channelSendBtn}
        disabled={!inputValue.trim() || sending}
        onClick={handleSend}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <line x1="22" y1="2" x2="11" y2="13" />
          <polygon points="22 2 15 22 11 13 2 9 22 2" />
        </svg>
      </button>
    </div>
  );
}

// ── ChannelReadonly

export function ChannelReadonly() {
  const isDM = useStore(s => s.channelIsDM);
  const currentChannel = useStore(s => s.currentChannel);

  if (!isDM || !currentChannel) return null;

  return (
    <span>
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ verticalAlign: 'middle', marginRight: 4 }}>
        <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
        <path d="M7 11V7a5 5 0 0 1 10 0v4" />
      </svg>
      {window.t?.('channel.readOnly') || '这是 Agent 之间的私信，仅可查看'}
    </span>
  );
}
