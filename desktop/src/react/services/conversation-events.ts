// 频道事件共用同一投影，旧消息通道和新服务事件都走这里。
/* eslint-disable @typescript-eslint/no-explicit-any -- 兼容既有事件载荷，入口逐项校验 */
import { useStore } from '../stores';
import {
  appendChannelMessage as appendChannelMessageAction,
  loadChannels as loadChannelsAction,
  markChannelMessagesDirty as markChannelMessagesDirtyAction,
  openChannel as openChannelAction,
  upsertConversationAgentActivity as upsertConversationAgentActivityAction,
} from '../stores/channel-actions';

function resolvePrimaryAgentId(state: any): string | null {
  const primary = Array.isArray(state.agents)
    ? state.agents.find((agent: any) => agent?.isPrimary === true)
    : null;
  return typeof primary?.id === 'string' && primary.id ? primary.id : null;
}

function resolveDmPeerIdForEvent(state: any, msg: any): string | null {
  const channels = Array.isArray(state.channels) ? state.channels : [];
  const known = channels.find((channel: any) => {
    if (!channel?.isDM || !channel.dmOwnerId || !channel.peerId) return false;
    return (
      (msg.from === channel.dmOwnerId && msg.to === channel.peerId)
      || (msg.to === channel.dmOwnerId && msg.from === channel.peerId)
    );
  });
  if (known?.peerId) return known.peerId;

  const ownerId = resolvePrimaryAgentId(state);
  if (!ownerId) return null;
  if (msg.from === ownerId && typeof msg.to === 'string') return msg.to;
  if (msg.to === ownerId && typeof msg.from === 'string') return msg.from;
  return null;
}


export function handleConversationEvent(msg: any): void {
  switch (msg.type) {
    case 'channel_new_message': {
      const store = useStore.getState();
      const knownChannel = store.channels.some((channel) => channel.id === msg.channelName);
      const isVisibleCurrentChannel =
        store.currentTab === 'channels'
        && store.currentChannel === msg.channelName
        && document.visibilityState === 'visible';
      if (typeof msg.channelName === 'string' && msg.message
        && typeof msg.message.sender === 'string' && typeof msg.message.timestamp === 'string' && typeof msg.message.body === 'string') {
        if (!knownChannel) loadChannelsAction();
        appendChannelMessageAction(msg.channelName, msg.message, { markRead: isVisibleCurrentChannel });
      } else if (msg.channelName && isVisibleCurrentChannel) {
        markChannelMessagesDirtyAction(msg.channelName);
        openChannelAction(msg.channelName);
      } else if (msg.channelName) {
        markChannelMessagesDirtyAction(msg.channelName);
        loadChannelsAction();
      }
      break;
    }

    case 'channel_created': {
      loadChannelsAction();
      break;
    }

    case 'dm_new_message': {
      const store2 = useStore.getState();
      const peerId = resolveDmPeerIdForEvent(store2, msg);
      if (!peerId) {
        loadChannelsAction();
        break;
      }
      const dmId = `dm:${peerId}`;
      const isViewingDM = store2.currentTab === 'channels' && store2.currentChannel === dmId && document.visibilityState === 'visible';
      if (isViewingDM) {
        openChannelAction(dmId, true);
      } else {
        loadChannelsAction();
      }
      break;
    }

    case 'conversation_agent_activity': {
      if (msg.activity && typeof msg.activity.conversationId === 'string' && typeof msg.activity.agentId === 'string'
        && typeof msg.activity.timestamp === 'string' && typeof msg.activity.state === 'string') {
        upsertConversationAgentActivityAction(msg.activity);
      }
      break;
    }

  }
}
