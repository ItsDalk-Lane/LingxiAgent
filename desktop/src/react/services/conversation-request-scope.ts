import { useStore } from '../stores';
import { resolveServerConnection } from './server-connection';

/** 请求只属于发起时的连接和助手；切走再切回也不能接受旧结果。 */
export function captureConversationRequestScope(includeSelection = false): {
  isCurrent: () => boolean;
  dispose: () => void;
} {
  const key = () => {
    const state = useStore.getState();
    return JSON.stringify([
      resolveServerConnection(state), state.currentAgentId,
      ...(includeSelection ? [state.currentChannel] : []),
    ]);
  };
  const initial = key();
  let current = true;
  const unsubscribe = typeof useStore.subscribe === 'function'
    ? useStore.subscribe(() => { if (key() !== initial) current = false; })
    : () => {};
  return {
    isCurrent: () => current && key() === initial,
    dispose: () => { current = false; unsubscribe(); },
  };
}
