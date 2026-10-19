/**
 * channel-actions.ts — Channel 副作用操作（网络请求 + 状态联动）
 *
 * 从 channel-slice.ts 提取，所有函数通过 useStore.getState() / useStore.setState() 访问 store。
 */

/* eslint-disable @typescript-eslint/no-explicit-any -- API 响应 JSON 及 catch(err: any) */

import { useStore } from './index';
import { lingxiFetch } from '../hooks/use-hana-fetch';
import { captureConversationRequestScope } from '../services/conversation-request-scope';
import { hasServerConnection, resolveServerConnection } from '../services/server-connection';
import type { AgentPhoneActivity, AgentPhoneSettings, AgentPhoneToolMode, Channel, ChannelAgentActivities, ChannelMessage } from '../types';

let listRequest = 0;
let historyRequest = 0;
let settingsRequest = 0;

const pendingUserActions = new Map<'message' | 'create', { signature: string; requestId: string }>();

function userActionRequest(kind: 'message' | 'create', payload: unknown): string {
  const state = useStore.getState();
  const signature = JSON.stringify([resolveServerConnection(state), state.currentAgentId, payload]);
  const previous = pendingUserActions.get(kind);
  if (previous?.signature === signature) return previous.requestId;
  const requestId = `channel-${kind}:${globalThis.crypto.randomUUID()}`;
  pendingUserActions.set(kind, { signature, requestId });
  return requestId;
}

function confirmUserAction(kind: 'message' | 'create', requestId: string): void {
  if (pendingUserActions.get(kind)?.requestId === requestId) pendingUserActions.delete(kind);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function reportFailure(key: string): void {
  const state = useStore.getState();
  state.addToast?.(typeof window !== 'undefined' && window.t ? window.t(key) : key, 'error');
}

async function responseData(response: Response): Promise<any> {
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const data = await response.json();
  if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Error('Invalid conversation response');
  if (data.error) throw new Error(String(data.error));
  return data;
}

function requireArray(value: unknown, field: string): any[] {
  if (!Array.isArray(value)) throw new Error(`Invalid conversation ${field}`);
  return value;
}

function requireMessages(value: unknown): ChannelMessage[] {
  const messages = requireArray(value, 'messages');
  if (messages.some(message => !message || ['sender', 'timestamp', 'body'].some(field => typeof message[field] !== 'string'))) {
    throw new Error('Invalid conversation message');
  }
  return messages;
}

// ══════════════════════════════════════════════════════
// 加载频道列表
// ══════════════════════════════════════════════════════

export async function loadChannels(): Promise<void> {
  if (!hasServerConnection(useStore.getState())) return;
  const scope = captureConversationRequestScope();
  const request = ++listRequest;
  const isCurrent = () => scope.isCurrent() && request === listRequest;
  useStore.setState({ channelListLoading: true, channelListError: null });
  try {
    if (useStore.getState().channelsEnabled === undefined) {
      const config = await responseData(await lingxiFetch('/api/config'));
      if (!isCurrent()) return;
      const enabled = config.channels?.enabled === true;
      useStore.setState({ channelsEnabled: enabled });
      if (!enabled) { useStore.setState({ channels: [], channelTotalUnread: 0 }); return; }
    }
    const [chRes, dmRes] = await Promise.all([
      lingxiFetch('/api/channels'),
      lingxiFetch('/api/dm'),
    ]);
    const [chData, dmData] = await Promise.all([responseData(chRes), responseData(dmRes)]);
    if (!isCurrent()) return;
    const channels: Channel[] = requireArray(chData.channels, 'channels').map((ch: any) => {
      if (typeof ch?.id !== 'string' || !ch.id) throw new Error('Invalid channel identity');
      return { ...ch, isDM: false };
    });
    const dms: Channel[] = requireArray(dmData.dms, 'dms').map((dm: any) => {
      if (typeof dm?.peerId !== 'string' || !dm.peerId) throw new Error('Invalid DM identity');
      return {
        id: `dm:${dm.peerId}`,
        name: dm.peerName || dm.peerId,
        members: [dm.peerId],
        lastMessage: dm.lastMessage || '',
        lastSender: dm.lastSender || '',
        lastTimestamp: dm.lastTimestamp || '',
        newMessageCount: 0,
        messageCount: dm.messageCount || 0,
        isDM: true,
        dmOwnerId: dm.ownerAgentId || dmData.ownerAgentId || undefined,
        peerId: dm.peerId,
        peerName: dm.peerName,
      };
    });
    const allChannels = [...channels, ...dms];
    const totalUnread = allChannels.reduce((sum, ch) => sum + (ch.newMessageCount || 0), 0);
    useStore.setState({ channels: allChannels, channelTotalUnread: totalUnread });
  } catch (error) {
    if (isCurrent()) useStore.setState({ channelListError: errorMessage(error) });
  } finally {
    if (isCurrent()) useStore.setState({ channelListLoading: false });
    scope.dispose();
  }
}

function keyActivities(activities: AgentPhoneActivity[]): Record<string, AgentPhoneActivity[]> {
  const keyed: Record<string, AgentPhoneActivity[]> = {};
  for (const activity of activities || []) {
    if (!activity?.agentId) continue;
    keyed[activity.agentId] = [activity];
  }
  return keyed;
}

export async function loadConversationAgentActivities(conversationId: string): Promise<void> {
  if (!conversationId || !hasServerConnection(useStore.getState())) return;
  const scope = captureConversationRequestScope();
  try {
    const res = await lingxiFetch(`/api/conversations/${encodeURIComponent(conversationId)}/agent-activities${conversationOwnerQuery(conversationId)}`);
    const data = await responseData(res);
    if (!scope.isCurrent()) return;
    const rows = requireArray(data.activities, 'activities');
    if (rows.some(activity => !activity || activity.conversationId !== conversationId
      || ['agentId', 'timestamp', 'state', 'summary'].some(key => typeof activity[key] !== 'string'))) {
      throw new Error('Invalid conversation activity');
    }
    const activities = keyActivities(rows);
    const state = useStore.getState();
    useStore.setState({
      channelAgentActivities: { ...state.channelAgentActivities, [conversationId]: activities },
      channelTickerStatus: { ...state.channelTickerStatus, [conversationId]: data.ticker || null },
    });
  } catch (error) {
    if (scope.isCurrent()) {
      console.error('[channels] load agent activities failed:', error);
      reportFailure('channel.loadFailed');
    }
  } finally {
    scope.dispose();
  }
}

export function upsertConversationAgentActivity(activity: AgentPhoneActivity): void {
  if (!activity?.conversationId || !activity.agentId) return;
  const state = useStore.getState();
  const current = (state.channelAgentActivities || {}) as ChannelAgentActivities;
  const byAgent = current[activity.conversationId] || {};
  const history = byAgent[activity.agentId] || [];
  const nextHistory = [
    activity,
    ...history.filter((item: AgentPhoneActivity) =>
      item.timestamp !== activity.timestamp || item.state !== activity.state || item.summary !== activity.summary),
  ].slice(0, 20);

  useStore.setState({
    channelAgentActivities: {
      ...current,
      [activity.conversationId]: {
        ...byAgent,
        [activity.agentId]: nextHistory,
      },
    },
  });
}

function normalizeAgentPhoneToolMode(mode: unknown): AgentPhoneToolMode {
  return mode === 'write' ? 'write' : 'read_only';
}

function conversationOwnerQuery(conversationId: string): string {
  if (!conversationId.startsWith('dm:')) return '';
  const channel = useStore.getState().channels.find((ch: Channel) => ch.id === conversationId);
  return channel?.dmOwnerId ? `?agentId=${encodeURIComponent(channel.dmOwnerId)}` : '';
}

function conversationPhoneSettingsUrl(conversationId: string): string {
  return `/api/conversations/${encodeURIComponent(conversationId)}/agent-phone-settings${conversationOwnerQuery(conversationId)}`;
}

function normalizeNullablePositiveInt(value: unknown): number | null {
  const num = Number(value);
  if (!Number.isFinite(num) || num <= 0) return null;
  return Math.floor(num);
}

function normalizeAgentPhoneSettings(data: any): AgentPhoneSettings {
  const overrideModel = data?.modelOverrideModel;
  return {
    mode: normalizeAgentPhoneToolMode(data?.mode),
    replyMinChars: normalizeNullablePositiveInt(data?.replyMinChars),
    replyMaxChars: normalizeNullablePositiveInt(data?.replyMaxChars),
    proactiveEnabled: data?.proactiveEnabled !== false,
    reminderIntervalMinutes: normalizeNullablePositiveInt(data?.reminderIntervalMinutes) || 31,
    guardLimit: normalizeNullablePositiveInt(data?.guardLimit) || 36,
    modelOverrideEnabled: data?.modelOverrideEnabled === true,
    modelOverrideModel: overrideModel?.id && overrideModel?.provider
      ? { id: String(overrideModel.id), provider: String(overrideModel.provider) }
      : null,
  };
}

function applyAgentPhoneSettings(settings: AgentPhoneSettings): void {
  useStore.setState({
    channelAgentPhoneToolMode: settings.mode,
    channelAgentReplyMinChars: settings.replyMinChars,
    channelAgentReplyMaxChars: settings.replyMaxChars,
    channelAgentProactiveEnabled: settings.proactiveEnabled,
    channelAgentReminderIntervalMinutes: settings.reminderIntervalMinutes,
    channelAgentGuardLimit: settings.guardLimit,
    channelAgentModelOverrideEnabled: settings.modelOverrideEnabled,
    channelAgentModelOverrideModel: settings.modelOverrideModel,
  });
}

function applyChannelMembers(channelId: string, members: string[]): void {
  const state = useStore.getState();
  const t = typeof window !== 'undefined' && window.t ? window.t : ((key: string) => key);
  const displayMembers = [state.userName || 'user', ...members];
  useStore.setState({
    channelMembers: state.currentChannel === channelId ? members : state.channelMembers,
    channelHeaderMembersText: state.currentChannel === channelId
      ? `${displayMembers.length} ${t('channel.membersCount')}`
      : state.channelHeaderMembersText,
    channels: state.channels.map((channel: Channel) =>
      channel.id === channelId ? { ...channel, members } : channel,
    ),
  });
}

export async function loadConversationAgentPhoneToolMode(conversationId: string): Promise<void> {
  await loadConversationAgentPhoneSettings(conversationId);
}

export async function loadConversationAgentPhoneSettings(conversationId: string): Promise<void> {
  if (!conversationId || !hasServerConnection(useStore.getState())) return;
  const scope = captureConversationRequestScope(true);
  const request = ++settingsRequest;
  const isCurrent = () => scope.isCurrent() && request === settingsRequest
    && useStore.getState().currentChannel === conversationId;
  if (isCurrent()) useStore.setState({ channelSettingsLoading: true, channelSettingsError: null });
  try {
    const data = await responseData(await lingxiFetch(conversationPhoneSettingsUrl(conversationId)));
    if (!isCurrent()) return;
    if (!['read_only', 'write'].includes(data.mode)) throw new Error('Invalid conversation settings');
    applyAgentPhoneSettings(normalizeAgentPhoneSettings(data));
  } catch (error) {
    if (isCurrent()) useStore.setState({ channelSettingsError: errorMessage(error) });
  } finally {
    if (isCurrent()) useStore.setState({ channelSettingsLoading: false });
    scope.dispose();
  }
}

export async function setConversationAgentPhoneToolMode(mode: AgentPhoneToolMode): Promise<void> {
  await saveConversationAgentPhoneSettings({ mode });
}

export async function saveConversationAgentPhoneSettings(patch: Partial<AgentPhoneSettings>): Promise<void> {
  const s = useStore.getState();
  const conversationId = s.currentChannel;
  if (!conversationId || !hasServerConnection(s)) return;
  const scope = captureConversationRequestScope(true);
  try {
    const res = await lingxiFetch(conversationPhoneSettingsUrl(conversationId), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        mode: patch.mode !== undefined ? normalizeAgentPhoneToolMode(patch.mode) : s.channelAgentPhoneToolMode,
        replyMinChars: patch.replyMinChars !== undefined ? patch.replyMinChars : s.channelAgentReplyMinChars,
        replyMaxChars: patch.replyMaxChars !== undefined ? patch.replyMaxChars : s.channelAgentReplyMaxChars,
        proactiveEnabled: patch.proactiveEnabled !== undefined ? patch.proactiveEnabled : s.channelAgentProactiveEnabled,
        reminderIntervalMinutes: patch.reminderIntervalMinutes !== undefined ? patch.reminderIntervalMinutes : s.channelAgentReminderIntervalMinutes,
        guardLimit: patch.guardLimit !== undefined ? patch.guardLimit : s.channelAgentGuardLimit,
        modelOverrideEnabled: patch.modelOverrideEnabled !== undefined ? patch.modelOverrideEnabled : s.channelAgentModelOverrideEnabled,
        modelOverrideModel: patch.modelOverrideModel !== undefined ? patch.modelOverrideModel : s.channelAgentModelOverrideModel,
      }),
    });
    const data = await responseData(res);
    if (!scope.isCurrent()) return;
    if (!['read_only', 'write'].includes(data.mode)) throw new Error('Invalid conversation settings');
    applyAgentPhoneSettings(normalizeAgentPhoneSettings(data));
  } finally {
    scope.dispose();
  }
}

// ══════════════════════════════════════════════════════
// 打开频道
// ══════════════════════════════════════════════════════

export async function openChannel(channelId: string, isDM?: boolean): Promise<void> {
  const state = useStore.getState();
  const channel = state.channels.find((item: Channel) => item.id === channelId);
  const isThisDM = channelId.startsWith('dm:') || isDM === true || channel?.isDM === true;
  const peerId = isThisDM ? (channel?.peerId || channelId.slice(3)) : '';
  const peerName = isThisDM ? (channel?.name || peerId) : '';
  const dmOwnerId = channel?.dmOwnerId;
  const request = ++historyRequest;
  // 缓存属于所选会话；尚未读到历史时明确显示加载或错误，不能当成空记录。
  useStore.setState({
    currentChannel: channelId,
    channelMessages: state.channelMessageCache[channelId] || [],
    channelMembers: isThisDM ? [peerId] : (channel?.members || []),
    channelHeaderName: isThisDM ? peerName : `# ${channel?.name || channelId}`,
    channelHeaderMembersText: '',
    channelIsDM: isThisDM,
    channelInfoName: isThisDM ? peerName : (channel?.name || channelId),
    channelHistoryLoading: true,
    channelHistoryError: null,
    channelSettingsLoading: true,
    channelSettingsError: null,
  });
  const scope = captureConversationRequestScope(true);
  const isCurrent = () => scope.isCurrent() && request === historyRequest;
  try {
    const ownerQuery = dmOwnerId ? `?agentId=${encodeURIComponent(dmOwnerId)}` : '';
    const path = isThisDM ? `/api/dm/${encodeURIComponent(peerId)}${ownerQuery}`
      : `/api/channels/${encodeURIComponent(channelId)}`;
    const data = await responseData(await lingxiFetch(path));
    if (!isCurrent()) return;
    const messages = requireMessages(data.messages);
    const members = isThisDM ? [peerId] : requireArray(data.members, 'members');
    if (members.some(member => typeof member !== 'string')) throw new Error('Invalid channel members');
    const fresh = useStore.getState();
    const name = isThisDM ? (data.peerName || peerName) : (data.name || channelId);
    const responseOwnerId = data.ownerAgentId || dmOwnerId;
    useStore.setState({
      channelMessages: messages,
      channelMessageCache: { ...fresh.channelMessageCache, [channelId]: messages },
      channelMessageCacheDirty: { ...fresh.channelMessageCacheDirty, [channelId]: false },
      channelMembers: members,
      channelHeaderName: isThisDM ? name : `# ${name}`,
      channelHeaderMembersText: isThisDM ? '' : `${members.length + 1} ${window.t('channel.membersCount')}`,
      channelInfoName: name,
      channels: isThisDM && responseOwnerId ? fresh.channels.map((item: Channel) =>
        item.id === channelId ? { ...item, dmOwnerId: responseOwnerId } : item) : fresh.channels,
    });
    const lastMessage = messages.at(-1);
    if (!isThisDM && lastMessage) {
      const readScope = captureConversationRequestScope(true);
      void lingxiFetch(`/api/channels/${encodeURIComponent(channelId)}/read`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ timestamp: lastMessage.timestamp }),
      }).then(response => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        if (!readScope.isCurrent()) return;
        const latest = useStore.getState();
        const unread = latest.channels.find(item => item.id === channelId)?.newMessageCount || 0;
        useStore.setState({
          channelTotalUnread: Math.max(0, latest.channelTotalUnread - unread),
          channels: latest.channels.map(item => item.id === channelId ? { ...item, newMessageCount: 0 } : item),
        });
      }).catch(error => console.warn('[channel-actions] mark-as-read failed', error)).finally(readScope.dispose);
    }
    void loadConversationAgentActivities(channelId);
    void loadConversationAgentPhoneSettings(channelId);
  } catch (error) {
    if (isCurrent()) useStore.setState({ channelHistoryError: errorMessage(error), channelSettingsError: errorMessage(error), channelSettingsLoading: false });
  } finally {
    if (isCurrent()) useStore.setState({ channelHistoryLoading: false });
    scope.dispose();
  }
}

function sameChannelMessage(a: ChannelMessage, b: ChannelMessage): boolean {
  return a.sender === b.sender && a.timestamp === b.timestamp && a.body === b.body;
}

function sortChannelsByRecent(channels: Channel[]): Channel[] {
  return [...channels].sort((a, b) =>
    (b.lastTimestamp || '').localeCompare(a.lastTimestamp || ''),
  );
}

function sameCachedMessage(a: ChannelMessage, b: ChannelMessage): boolean {
  return sameChannelMessage(a, b);
}

export function markChannelMessagesDirty(channelId: string): void {
  if (!channelId) return;
  const state = useStore.getState();
  useStore.setState({
    channelMessageCacheDirty: {
      ...state.channelMessageCacheDirty,
      [channelId]: true,
    },
  });
}

export async function hydrateCurrentChannelIfNeeded(): Promise<void> {
  const state = useStore.getState();
  const channelId = state.currentChannel;
  if (!channelId) return;

  const cached = state.channelMessageCache[channelId];
  const dirty = state.channelMessageCacheDirty[channelId] === true;
  if (cached) {
    useStore.setState({ channelMessages: cached });
  }
  if (!cached || dirty) {
    const channel = state.channels.find((item: Channel) => item.id === channelId);
    await openChannel(channelId, channel?.isDM);
  }
}

// ══════════════════════════════════════════════════════
// 增量追加频道消息
// ══════════════════════════════════════════════════════

export function appendChannelMessage(
  channelId: string,
  message: ChannelMessage,
  options: { markRead?: boolean; countUnread?: boolean } = { markRead: true },
): void {
  if (
    !channelId
    || typeof message?.sender !== 'string'
    || typeof message.timestamp !== 'string'
    || typeof message.body !== 'string'
  ) return;

  const state = useStore.getState();
  const isCurrentChannel = state.currentChannel === channelId;
  const cachedMessages = state.channelMessageCache[channelId];
  const baseMessages = cachedMessages || (isCurrentChannel ? state.channelMessages : []);
  const alreadyInCache = baseMessages.some((m: ChannelMessage) => sameCachedMessage(m, message));
  const nextMessages = alreadyInCache ? baseMessages : [...baseMessages, message];
  const shouldMarkRead = isCurrentChannel && options.markRead === true;
  const shouldCountUnread = !shouldMarkRead && options.countUnread !== false;
  const nextCacheDirty = state.channelMessageCacheDirty[channelId] === true
    || (!cachedMessages && !isCurrentChannel);

  let unreadDelta = 0;
  let readDelta = 0;
  const updatedChannels = state.channels.map((channel: Channel) => {
    if (channel.id !== channelId) return channel;

    const isDuplicatePreview =
      channel.lastSender === message.sender
      && channel.lastTimestamp === message.timestamp
      && channel.lastMessage === message.body.slice(0, 60);

    const previousUnread = channel.newMessageCount || 0;
    const nextUnread = shouldMarkRead ? 0 : previousUnread + (
      shouldCountUnread && !isDuplicatePreview ? 1 : 0
    );

    if (shouldMarkRead) {
      readDelta = previousUnread;
    } else {
      unreadDelta += nextUnread - previousUnread;
    }

    return {
      ...channel,
      lastMessage: message.body.slice(0, 60),
      lastSender: message.sender,
      lastTimestamp: message.timestamp,
      messageCount: (channel.messageCount || 0) + (isDuplicatePreview ? 0 : 1),
      newMessageCount: nextUnread,
    };
  });

  const patch: Partial<ReturnType<typeof useStore.getState>> = {
    channels: sortChannelsByRecent(updatedChannels),
    channelMessageCache: {
      ...state.channelMessageCache,
      [channelId]: nextMessages,
    },
    channelMessageCacheDirty: {
      ...state.channelMessageCacheDirty,
      [channelId]: nextCacheDirty,
    },
    channelTotalUnread: Math.max(0, state.channelTotalUnread + unreadDelta - readDelta),
  };

  if (isCurrentChannel) {
    patch.channelMessages = nextMessages;
  }

  useStore.setState(patch);

  if (shouldMarkRead) {
    Promise.resolve(lingxiFetch(`/api/channels/${encodeURIComponent(channelId)}/read`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ timestamp: message.timestamp }),
    })).catch((err: unknown) => console.warn('[channel-actions] mark-as-read failed', err));
  }
}

// ══════════════════════════════════════════════════════
// 发送消息
// ══════════════════════════════════════════════════════

export async function sendChannelMessage(text: string): Promise<void> {
  const s = useStore.getState();
  const channelId = s.currentChannel;
  const body = text.trim();
  if (!body || !channelId) return;
  if (channelId.startsWith('dm:') || s.channelIsDM || s.channels.some(channel => channel.id === channelId && channel.isDM)) {
    throw new Error('Agent DM is read only');
  }
  const scope = captureConversationRequestScope();
  const sender = s.userName || 'user';
  const requestId = userActionRequest('message', { channelId, body: text });

  try {
    const res = await lingxiFetch(`/api/channels/${encodeURIComponent(channelId)}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ body: text, requestId }),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    if (!data.ok || typeof data.timestamp !== 'string') throw new Error('Message was not accepted');
    confirmUserAction('message', requestId);
    if (!scope.isCurrent()) return;
    if (data.ok) {
      appendChannelMessage(channelId, {
        sender,
        timestamp: data.timestamp,
        body: text,
      }, { markRead: true, countUnread: false });
    }
  } catch (err) {
    if (scope.isCurrent()) throw err;
  } finally {
    scope.dispose();
  }
}

function exportedArchiveFilename(disposition: string | null): string {
  const encoded = disposition?.match(/filename\*=UTF-8''([^;]+)/i)?.[1];
  if (encoded) {
    try { return decodeURIComponent(encoded); } catch { /* use fallback */ }
  }
  const plain = disposition?.match(/filename="([^"]+)"/i)?.[1];
  return plain || 'hana-chat-archive.md';
}

export async function exportCurrentConversation(): Promise<string> {
  const conversationId = useStore.getState().currentChannel;
  if (!conversationId) throw new Error('No active conversation');
  const scope = captureConversationRequestScope(true);
  try {
    const ownerQuery = conversationOwnerQuery(conversationId);
    const res = await lingxiFetch(`/api/conversations/${encodeURIComponent(conversationId)}/export${ownerQuery}`);
    if (!res.ok) {
      const payload = await res.json().catch(() => null);
      throw new Error(payload?.error || `HTTP ${res.status}`);
    }

    const blob = await res.blob();
    if (!scope.isCurrent()) throw new Error('Conversation selection changed');
    const filename = exportedArchiveFilename(res.headers.get('content-disposition'));
    const downloadUrl = URL.createObjectURL(blob);
    try {
      const anchor = document.createElement('a');
      anchor.href = downloadUrl;
      anchor.download = filename;
      anchor.rel = 'noopener';
      anchor.click();
    } finally {
      URL.revokeObjectURL(downloadUrl);
    }
    return filename;
  } finally { scope.dispose(); }
}

// ══════════════════════════════════════════════════════
// 删除频道
// ══════════════════════════════════════════════════════

export async function deleteChannel(channelId: string): Promise<void> {
  const scope = captureConversationRequestScope();
  try {
    const res = await lingxiFetch(`/api/channels/${encodeURIComponent(channelId)}`, {
      method: 'DELETE',
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    if (!scope.isCurrent()) return;
    if (data.ok) {
      if (useStore.getState().currentChannel === channelId) {
        useStore.setState({
          currentChannel: null,
          channelMessages: [],
          channelHeaderName: '',
          channelHeaderMembersText: '',
          channelIsDM: false,
        });
      }
      // Reload channels
      await loadChannels();
    } else {
      throw new Error(data.error || 'Channel was not deleted');
    }
  } catch {
    if (scope.isCurrent()) reportFailure('channel.deleteFailed');
  } finally {
    scope.dispose();
  }
}

// ══════════════════════════════════════════════════════
// 频道成员管理
// ══════════════════════════════════════════════════════

export async function addChannelMember(channelId: string, memberId: string): Promise<void> {
  const scope = captureConversationRequestScope();
  try {
    const res = await lingxiFetch(`/api/channels/${encodeURIComponent(channelId)}/members`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ memberId }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok || data.error) {
      throw new Error(data.error || `HTTP ${res.status}`);
    }
    if (!scope.isCurrent()) return;
    const members = requireArray(data.members, 'members');
    if (members.some(member => typeof member !== 'string')) throw new Error('Invalid channel members');
    applyChannelMembers(channelId, members);
  } finally { scope.dispose(); }
}

export async function removeChannelMember(channelId: string, memberId: string): Promise<void> {
  const scope = captureConversationRequestScope();
  try {
    const res = await lingxiFetch(`/api/channels/${encodeURIComponent(channelId)}/members/${encodeURIComponent(memberId)}`, {
      method: 'DELETE',
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok || data.error) {
      throw new Error(data.error || `HTTP ${res.status}`);
    }
    if (!scope.isCurrent()) return;
    const members = requireArray(data.members, 'members');
    if (members.some(member => typeof member !== 'string')) throw new Error('Invalid channel members');
    applyChannelMembers(channelId, members);
  } finally { scope.dispose(); }
}

// ══════════════════════════════════════════════════════
// 切换频道功能开关
// ══════════════════════════════════════════════════════

export async function toggleChannelsEnabled(): Promise<boolean | undefined> {
  const s = useStore.getState();
  if (s.channelsEnabled === undefined) return undefined;
  const previousEnabled = s.channelsEnabled;
  const newEnabled = !s.channelsEnabled;

  const scope = captureConversationRequestScope();
  try {
    const res = await lingxiFetch('/api/channels/toggle', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled: newEnabled }),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json().catch(() => ({}));
    if (!scope.isCurrent()) return undefined;
    const enabled = typeof data.enabled === 'boolean' ? data.enabled : newEnabled;
    useStore.setState({ channelsEnabled: enabled });

    if (enabled) {
      await loadChannels();
    } else {
      useStore.setState({
        channelListError: null,
        channelHistoryError: null,
        channelSettingsError: null,
        channels: [],
        currentChannel: null,
        channelMessages: [],
        channelMessageCache: {},
        channelMessageCacheDirty: {},
        channelMembers: [],
        channelTotalUnread: 0,
        channelHeaderName: '',
        channelHeaderMembersText: '',
        channelInfoName: '',
        channelIsDM: false,
      });
    }

    return enabled;
  } catch {
    if (scope.isCurrent()) reportFailure('channel.settingsSaveFailed');
    return scope.isCurrent() ? previousEnabled : undefined;
  } finally {
    scope.dispose();
  }
}

// ══════════════════════════════════════════════════════
// 创建频道
// ══════════════════════════════════════════════════════

export async function createChannel(name: string, members: string[], intro?: string): Promise<string | null> {
  const scope = captureConversationRequestScope(true);
  const requestId = userActionRequest('create', { name, members, intro: intro || null });
  try {
    const res = await lingxiFetch('/api/channels', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        requestId,
        name,
        members,
        intro: intro || undefined,
      }),
      throwOnHttpError: false,
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
    if (data.error) throw new Error(data.error);
    if (typeof data.id !== 'string' || !data.id) throw new Error('Channel creation was not confirmed');
    confirmUserAction('create', requestId);

    if (!scope.isCurrent()) return null;
    await loadChannels();
    if (!scope.isCurrent()) return null;
    if (data.id) {
      await openChannel(data.id);
    }
    return data.id || null;
  } catch (err: any) {
    console.error('[channels] create failed:', err);
    if (scope.isCurrent()) throw err;
    return null;
  } finally {
    scope.dispose();
  }
}
