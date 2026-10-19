import { beforeEach, describe, expect, it } from 'vitest';
import { useStore } from '../../stores';
import { captureConversationRequestScope } from '../../services/conversation-request-scope';

describe('频道请求的连接与助手范围', () => {
  beforeEach(() => {
    useStore.setState({ serverPort: '3210', activeServerConnectionId: null, activeServerConnection: null,
      currentAgentId: 'a', currentChannel: 'ch1' });
  });

  it.each(['currentAgentId', 'serverPort', 'currentChannel'] as const)('切换%s再切回来也不接受旧结果', field => {
    const scope = captureConversationRequestScope(true);
    const previous = useStore.getState()[field];
    useStore.setState({ [field]: field === 'serverPort' ? '3211' : 'other' });
    useStore.setState({ [field]: previous });
    expect(scope.isCurrent()).toBe(false);
    scope.dispose();
  });

  it('列表请求允许切频道，结束后释放监听并失效', () => {
    const scope = captureConversationRequestScope();
    useStore.setState({ currentChannel: 'ch2' });
    expect(scope.isCurrent()).toBe(true);
    scope.dispose();
    expect(scope.isCurrent()).toBe(false);
  });
});
