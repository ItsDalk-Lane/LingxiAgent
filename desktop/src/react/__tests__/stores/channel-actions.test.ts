/**
 * channel-actions 基线测试
 *
 * 测试纯逻辑部分（不涉及网络请求的函数），
 * 以及 store 状态变化的正确性。
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock store
const mockState: Record<string, unknown> = {
  serverPort: '3210',
  channels: [],
  currentChannel: null,
  channelMessages: [],
  channelMessageCache: {},
  channelMessageCacheDirty: {},
  channelTotalUnread: 0,
  channelsEnabled: true,
  userName: 'testuser',
  channelMembers: [],
  channelHeaderName: '',
  channelHeaderMembersText: '',
  channelIsDM: false,
  channelInfoName: '',
  channelAgentActivities: {},
  channelAgentPhoneToolMode: 'read_only',
  channelAgentReplyMinChars: null,
  channelAgentReplyMaxChars: null,
  channelAgentProactiveEnabled: true,
  channelAgentReminderIntervalMinutes: 31,
  channelAgentGuardLimit: 36,
  channelAgentModelOverrideEnabled: false,
  channelAgentModelOverrideModel: null,
};

const setStateCalls: Array<Record<string, unknown>> = [];

vi.mock('../../stores', () => ({
  useStore: {
    getState: () => ({ ...mockState }),
    setState: (patch: Record<string, unknown>) => {
      setStateCalls.push(patch);
      Object.assign(mockState, patch);
    },
  },
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: vi.fn(),
}));

import { lingxiFetch } from '../../hooks/use-hana-fetch';

const mockFetch = vi.mocked(lingxiFetch);

describe('channel-actions', () => {
  beforeEach(() => {
    setStateCalls.length = 0;
    mockState.serverPort = '3210';
    mockState.currentAgentId = 'hana';
    mockState.activeServerConnection = null;
    mockState.activeServerConnectionId = null;
    mockState.channelIsDM = false;
    mockState.channelListError = null;
    mockState.channelListLoading = false;
    mockState.channelHistoryError = null;
    mockState.channelHistoryLoading = false;
    mockState.channelSettingsError = null;
    mockState.channelSettingsLoading = false;
    mockState.channels = [];
    mockState.currentChannel = null;
    mockState.channelMessages = [];
    mockState.channelMessageCache = {};
    mockState.channelMessageCacheDirty = {};
    mockState.channelTotalUnread = 0;
    mockState.channelsEnabled = true;
    mockState.channelAgentPhoneToolMode = 'read_only';
    mockState.channelAgentReplyMinChars = null;
    mockState.channelAgentReplyMaxChars = null;
    mockState.channelAgentProactiveEnabled = true;
    mockState.channelAgentReminderIntervalMinutes = 31;
    mockState.channelAgentGuardLimit = 36;
    mockState.channelAgentModelOverrideEnabled = false;
    mockState.channelAgentModelOverrideModel = null;
    mockFetch.mockReset();
  });

  describe('loadChannels', () => {
    it('加载频道和 DM 列表', async () => {
      mockFetch
        .mockResolvedValueOnce({
          ok: true,
          json: async () => ({ channels: [{ id: 'ch1', name: 'general', newMessageCount: 2 }] }),
        } as Response)
        .mockResolvedValueOnce({
          ok: true,
          json: async () => ({ ownerAgentId: 'hana', dms: [{ ownerAgentId: 'hana', peerId: 'agent1', peerName: 'Agent 1', messageCount: 5 }] }),
        } as Response);

      const { loadChannels } = await import('../../stores/channel-actions');
      await loadChannels();

      expect(mockFetch).toHaveBeenCalledTimes(2);
      // 检查 setState 被调用，包含合并的 channels
      const lastPatch = setStateCalls.findLast(patch => patch.channels)!;
      expect(lastPatch.channels).toBeDefined();
      const channels = lastPatch.channels as Array<{ id: string; isDM: boolean; dmOwnerId?: string }>;
      expect(channels.length).toBe(2);
      expect(channels[0].isDM).toBe(false);
      expect(channels[1].isDM).toBe(true);
      expect(channels[1].id).toBe('dm:agent1');
      expect(channels[1].dmOwnerId).toBe('hana');
    });

    it('serverPort 为空时不请求', async () => {
      mockState.serverPort = '';
      const { loadChannels } = await import('../../stores/channel-actions');
      await loadChannels();
      expect(mockFetch).not.toHaveBeenCalled();
      mockState.serverPort = '3210';
    });
  });

  describe('openChannel', () => {
    it('opens DM history with the stored owner agent id', async () => {
      vi.stubGlobal('window', { t: (key: string) => key });
      mockState.channels = [{
        id: 'dm:agent1',
        name: 'Agent 1',
        members: ['agent1'],
        lastMessage: '',
        lastSender: '',
        lastTimestamp: '',
        newMessageCount: 0,
        isDM: true,
        peerId: 'agent1',
        peerName: 'Agent 1',
        dmOwnerId: 'hana',
      }];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          ownerAgentId: 'hana',
          peerId: 'agent1',
          peerName: 'Agent 1',
          messages: [{ sender: 'agent1', timestamp: '2026-05-19 12:00:00', body: 'hello' }],
        }),
      } as Response)
        .mockResolvedValueOnce({
          ok: true,
          json: async () => ({ activities: [] }),
        } as Response)
        .mockResolvedValueOnce({
          ok: true,
          json: async () => ({ mode: 'read_only' }),
        } as Response);

      const { openChannel } = await import('../../stores/channel-actions');
      await openChannel('dm:agent1', true);

      expect(mockFetch).toHaveBeenCalledWith('/api/dm/agent1?agentId=hana');
      expect(mockState.channelMessages).toEqual([
        { sender: 'agent1', timestamp: '2026-05-19 12:00:00', body: 'hello' },
      ]);
    });
  });

  describe('loadConversationAgentActivities', () => {
    it('loads and keys agent phone activities by conversation and agent', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          activities: [{
            conversationId: 'ch1',
            conversationType: 'channel',
            agentId: 'hana',
            state: 'idle',
            summary: '已回复',
            timestamp: '2026-05-12T12:00:00.000Z',
          }],
        }),
      } as Response);

      const { loadConversationAgentActivities } = await import('../../stores/channel-actions');
      await loadConversationAgentActivities('ch1');

      expect(mockFetch).toHaveBeenCalledWith('/api/conversations/ch1/agent-activities');
      expect((mockState.channelAgentActivities as any).ch1.hana[0]).toMatchObject({
        state: 'idle',
        summary: '已回复',
      });
    });
  });

  describe('DM phone settings owner', () => {
    it('loads DM phone settings with the stored owner agent id', async () => {
      mockState.currentChannel = 'dm:agent1';
      mockState.channels = [{
        id: 'dm:agent1',
        name: 'Agent 1',
        members: ['agent1'],
        lastMessage: '',
        lastSender: '',
        lastTimestamp: '',
        newMessageCount: 0,
        isDM: true,
        peerId: 'agent1',
        peerName: 'Agent 1',
        dmOwnerId: 'hana',
      }];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ mode: 'write', replyMinChars: 10, replyMaxChars: 80 }),
      } as Response);

      const { loadConversationAgentPhoneSettings } = await import('../../stores/channel-actions');
      await loadConversationAgentPhoneSettings('dm:agent1');

      expect(mockFetch).toHaveBeenCalledWith('/api/conversations/dm%3Aagent1/agent-phone-settings?agentId=hana');
      expect(mockState.channelAgentPhoneToolMode).toBe('write');
      expect(mockState.channelAgentReplyMinChars).toBe(10);
      expect(mockState.channelAgentReplyMaxChars).toBe(80);
    });

    it('saves DM phone settings with the stored owner agent id', async () => {
      mockState.currentChannel = 'dm:agent1';
      mockState.channels = [{
        id: 'dm:agent1',
        name: 'Agent 1',
        members: ['agent1'],
        lastMessage: '',
        lastSender: '',
        lastTimestamp: '',
        newMessageCount: 0,
        isDM: true,
        peerId: 'agent1',
        peerName: 'Agent 1',
        dmOwnerId: 'hana',
      }];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ mode: 'write', replyMinChars: 20, replyMaxChars: 90 }),
      } as Response);

      const { saveConversationAgentPhoneSettings } = await import('../../stores/channel-actions');
      await saveConversationAgentPhoneSettings({ mode: 'write', replyMinChars: 20, replyMaxChars: 90 });

      expect(mockFetch).toHaveBeenCalledWith('/api/conversations/dm%3Aagent1/agent-phone-settings?agentId=hana', expect.objectContaining({
        method: 'POST',
      }));
      expect(mockState.channelAgentPhoneToolMode).toBe('write');
    });
  });

  describe('setConversationAgentPhoneToolMode', () => {
    it('persists and updates the current conversation phone tool mode', async () => {
      mockState.currentChannel = 'ch1';
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, mode: 'write' }),
      } as Response);

      const { setConversationAgentPhoneToolMode } = await import('../../stores/channel-actions');
      await setConversationAgentPhoneToolMode('write');

      expect(mockFetch).toHaveBeenCalledWith('/api/conversations/ch1/agent-phone-settings', expect.objectContaining({
        method: 'POST',
      }));
      expect(mockState.channelAgentPhoneToolMode).toBe('write');
    });

    it('persists reply range settings without changing API output budget', async () => {
      mockState.currentChannel = 'ch1';
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          ok: true,
          mode: 'read_only',
          replyMinChars: 20,
          replyMaxChars: 80,
          proactiveEnabled: false,
          reminderIntervalMinutes: 45,
          guardLimit: 9,
          modelOverrideEnabled: true,
          modelOverrideModel: { id: 'deepseek-v4-flash', provider: 'deepseek' },
        }),
      } as Response);

      const { saveConversationAgentPhoneSettings } = await import('../../stores/channel-actions');
      await saveConversationAgentPhoneSettings({
        replyMinChars: 20,
        replyMaxChars: 80,
        proactiveEnabled: false,
        reminderIntervalMinutes: 45,
        guardLimit: 9,
        modelOverrideEnabled: true,
        modelOverrideModel: { id: 'deepseek-v4-flash', provider: 'deepseek' },
      });

      const [, init] = mockFetch.mock.calls[0];
      expect(mockFetch.mock.calls[0][0]).toBe('/api/conversations/ch1/agent-phone-settings');
      const body = JSON.parse(String((init as RequestInit).body));
      expect(body).toMatchObject({
        replyMinChars: 20,
        replyMaxChars: 80,
        proactiveEnabled: false,
        reminderIntervalMinutes: 45,
        guardLimit: 9,
        modelOverrideEnabled: true,
        modelOverrideModel: { id: 'deepseek-v4-flash', provider: 'deepseek' },
      });
      expect(body).not.toHaveProperty('replyInstructions');
      expect(body).not.toHaveProperty('maxTokens');
      expect(mockState.channelAgentReplyMinChars).toBe(20);
      expect(mockState.channelAgentReplyMaxChars).toBe(80);
      expect(mockState.channelAgentProactiveEnabled).toBe(false);
      expect(mockState.channelAgentReminderIntervalMinutes).toBe(45);
      expect(mockState.channelAgentGuardLimit).toBe(9);
      expect(mockState.channelAgentModelOverrideEnabled).toBe(true);
      expect(mockState.channelAgentModelOverrideModel).toEqual({ id: 'deepseek-v4-flash', provider: 'deepseek' });
    });
  });

  describe('channel member management', () => {
    it('adds a member and updates the current channel projection', async () => {
      mockState.currentChannel = 'ch1';
      mockState.userName = 'testuser';
      mockState.channelMembers = ['hana', 'butter'];
      mockState.channels = [{
        id: 'ch1',
        name: 'general',
        members: ['hana', 'butter'],
        lastMessage: '',
        lastSender: '',
        lastTimestamp: '',
        newMessageCount: 0,
      }];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, members: ['hana', 'butter', 'ming'] }),
      } as Response);

      const { addChannelMember } = await import('../../stores/channel-actions');
      await addChannelMember('ch1', 'ming');

      expect(mockFetch).toHaveBeenCalledWith('/api/channels/ch1/members', expect.objectContaining({
        method: 'POST',
      }));
      expect(mockState.channelMembers).toEqual(['hana', 'butter', 'ming']);
      expect((mockState.channels as any[])[0].members).toEqual(['hana', 'butter', 'ming']);
      expect(mockState.channelHeaderMembersText).toBe('4 channel.membersCount');
    });

    it('surfaces backend member removal errors without mutating local members', async () => {
      mockState.currentChannel = 'ch1';
      mockState.channelMembers = ['hana', 'butter'];
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 400,
        json: async () => ({ error: 'channel requires at least 2 agent members' }),
      } as Response);

      const { removeChannelMember } = await import('../../stores/channel-actions');
      await expect(removeChannelMember('ch1', 'butter')).rejects.toThrow(/at least 2/i);
      expect(mockState.channelMembers).toEqual(['hana', 'butter']);
    });
  });

  describe('sendChannelMessage', () => {
    it('空消息不发送', async () => {
      mockState.currentChannel = 'ch1';
      const { sendChannelMessage } = await import('../../stores/channel-actions');
      await sendChannelMessage('   ');
      expect(mockFetch).not.toHaveBeenCalled();
    });

    it('无当前频道不发送', async () => {
      mockState.currentChannel = null;
      const { sendChannelMessage } = await import('../../stores/channel-actions');
      await sendChannelMessage('hello');
      expect(mockFetch).not.toHaveBeenCalled();
    });

    it('发送成功后追加消息到 store', async () => {
      mockState.currentChannel = 'ch1';
      mockState.channelMessages = [];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, timestamp: '2026-03-22T00:00:00Z' }),
      } as Response);

      const { sendChannelMessage } = await import('../../stores/channel-actions');
      await sendChannelMessage('hello world');

      const msgPatch = setStateCalls.find(p => p.channelMessages);
      expect(msgPatch).toBeDefined();
      const msgs = msgPatch!.channelMessages as Array<{ sender: string; body: string }>;
      expect(msgs[msgs.length - 1].body).toBe('hello world');
      expect(msgs[msgs.length - 1].sender).toBe('testuser');
    });

    it('发送成功后按发送开始时的频道同步消息 cache 和频道列表投影', async () => {
      mockState.currentChannel = 'ch1';
      mockState.channelMessages = [
        { sender: 'lingxi', timestamp: '2026-05-07 17:00:00', body: 'old ch1' },
      ];
      mockState.channelMessageCache = {
        ch1: mockState.channelMessages,
        ch2: [{ sender: 'ming', timestamp: '2026-05-07 16:00:00', body: 'old ch2' }],
      };
      mockState.channelMessageCacheDirty = { ch1: false, ch2: false };
      mockState.channels = [{
        id: 'ch1',
        name: 'general',
        members: [],
        lastMessage: 'old ch1',
        lastSender: 'lingxi',
        lastTimestamp: '2026-05-07 17:00:00',
        messageCount: 1,
        newMessageCount: 0,
        isDM: false,
      }, {
        id: 'ch2',
        name: 'random',
        members: [],
        lastMessage: 'old ch2',
        lastSender: 'ming',
        lastTimestamp: '2026-05-07 16:00:00',
        messageCount: 1,
        newMessageCount: 0,
        isDM: false,
      }];

      let resolveSend!: (value: Response) => void;
      mockFetch.mockReturnValueOnce(new Promise<Response>((resolve) => {
        resolveSend = resolve;
      }));

      const { sendChannelMessage } = await import('../../stores/channel-actions');
      const pendingSend = sendChannelMessage('hello from me');

      mockState.currentChannel = 'ch2';
      mockState.channelMessages = (mockState.channelMessageCache as any).ch2;
      resolveSend({
        ok: true,
        json: async () => ({ ok: true, timestamp: '2026-05-07 17:01:00' }),
      } as Response);
      await pendingSend;

      expect(mockFetch).toHaveBeenCalledWith('/api/channels/ch1/messages', expect.objectContaining({
        method: 'POST',
      }));
      expect(mockState.channelMessages).toEqual([
        { sender: 'ming', timestamp: '2026-05-07 16:00:00', body: 'old ch2' },
      ]);
      expect((mockState.channelMessageCache as any).ch1).toEqual([
        { sender: 'lingxi', timestamp: '2026-05-07 17:00:00', body: 'old ch1' },
        { sender: 'testuser', timestamp: '2026-05-07 17:01:00', body: 'hello from me' },
      ]);
      expect((mockState.channelMessageCacheDirty as any).ch1).toBe(false);
      expect((mockState.channels as any[])[0]).toMatchObject({
        id: 'ch1',
        lastMessage: 'hello from me',
        lastSender: 'testuser',
        lastTimestamp: '2026-05-07 17:01:00',
        messageCount: 2,
        newMessageCount: 0,
      });
    });
  });

  describe('exportCurrentConversation', () => {
    it('downloads the Markdown response using the server-provided filename', async () => {
      mockState.currentChannel = 'ch_family';
      const blob = new Blob(['# Archive'], { type: 'text/markdown' });
      mockFetch.mockResolvedValueOnce({
        ok: true,
        blob: async () => blob,
        headers: new Headers({
          'content-disposition': "attachment; filename=\"archive.md\"; filename*=UTF-8''hana-chat-archive.md",
        }),
      } as Response);
      const createObjectURL = vi.fn(() => 'blob:archive');
      const revokeObjectURL = vi.fn();
      Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: createObjectURL });
      Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: revokeObjectURL });
      const click = vi.fn();
      const anchor = { href: '', download: '', rel: '', click };
      const originalDocument = globalThis.document;
      Object.defineProperty(globalThis, 'document', {
        configurable: true,
        value: { createElement: vi.fn(() => anchor) },
      });

      try {
        const { exportCurrentConversation } = await import('../../stores/channel-actions');
        await expect(exportCurrentConversation()).resolves.toBe('hana-chat-archive.md');
        expect(mockFetch).toHaveBeenCalledWith('/api/conversations/ch_family/export');
        expect(click).toHaveBeenCalledOnce();
        expect(revokeObjectURL).toHaveBeenCalledWith('blob:archive');
      } finally {
        if (originalDocument) {
          Object.defineProperty(globalThis, 'document', { configurable: true, value: originalDocument });
        } else {
          Reflect.deleteProperty(globalThis, 'document');
        }
      }
    });

    it('exports the current DM from its stored owner Agent', async () => {
      mockState.currentChannel = 'dm:bob';
      mockState.channels = [{ id: 'dm:bob', isDM: true, dmOwnerId: 'alice', peerId: 'bob' }];
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 500,
        json: async () => ({ error: 'stop after URL assertion' }),
      } as Response);

      const { exportCurrentConversation } = await import('../../stores/channel-actions');
      await expect(exportCurrentConversation()).rejects.toThrow('stop after URL assertion');
      expect(mockFetch).toHaveBeenCalledWith('/api/conversations/dm%3Abob/export?agentId=alice');
    });
  });

  describe('appendChannelMessage', () => {
    it('追加当前频道的新消息并刷新频道预览，不清空已有消息', async () => {
      mockState.currentTab = 'channels';
      mockState.currentChannel = 'ch1';
      mockState.channelMessages = [
        { sender: 'testuser', timestamp: '2026-05-07 17:00:00', body: 'old' },
      ];
      mockState.channels = [{
        id: 'ch1',
        name: 'general',
        members: [],
        lastMessage: 'old',
        lastSender: 'testuser',
        lastTimestamp: '2026-05-07 17:00:00',
        newMessageCount: 3,
        isDM: false,
      }];
      mockState.channelTotalUnread = 3;

      const { appendChannelMessage } = await import('../../stores/channel-actions');
      appendChannelMessage('ch1', {
        sender: 'lingxi',
        timestamp: '2026-05-07 17:01:00',
        body: 'new reply',
      });

      expect(mockState.channelMessages).toEqual([
        { sender: 'testuser', timestamp: '2026-05-07 17:00:00', body: 'old' },
        { sender: 'lingxi', timestamp: '2026-05-07 17:01:00', body: 'new reply' },
      ]);
      expect((mockState.channels as Array<{ lastMessage: string; newMessageCount: number }>)[0]).toMatchObject({
        lastMessage: 'new reply',
        newMessageCount: 0,
      });
      expect(mockState.channelTotalUnread).toBe(0);
    });

    it('updates the current channel body cache while chat tab is active without marking read', async () => {
      mockState.currentTab = 'chat';
      mockState.currentChannel = 'ch1';
      mockState.channelMessages = [
        { sender: 'testuser', timestamp: '2026-05-07 17:00:00', body: 'old' },
      ];
      mockState.channels = [{
        id: 'ch1',
        name: 'general',
        members: [],
        lastMessage: 'old',
        lastSender: 'testuser',
        lastTimestamp: '2026-05-07 17:00:00',
        newMessageCount: 0,
        isDM: false,
      }];

      const { appendChannelMessage, hydrateCurrentChannelIfNeeded } = await import('../../stores/channel-actions');
      appendChannelMessage('ch1', {
        sender: 'lingxi',
        timestamp: '2026-05-07 17:01:00',
        body: 'new reply',
      }, { markRead: false });

      expect(mockState.channelMessages).toEqual([
        { sender: 'testuser', timestamp: '2026-05-07 17:00:00', body: 'old' },
        { sender: 'lingxi', timestamp: '2026-05-07 17:01:00', body: 'new reply' },
      ]);
      expect((mockState.channelMessageCache as any).ch1).toEqual(mockState.channelMessages);
      expect((mockState.channels as Array<{ newMessageCount: number }>)[0].newMessageCount).toBe(1);
      expect(mockFetch).not.toHaveBeenCalledWith('/api/channels/ch1/read', expect.anything());

      mockState.currentTab = 'channels';
      await hydrateCurrentChannelIfNeeded();

      expect(mockFetch).not.toHaveBeenCalledWith('/api/channels/ch1', expect.anything());
      expect(mockState.channelMessages).toEqual((mockState.channelMessageCache as any).ch1);
    });

    it('does not mark the current channel as read when the document is hidden', async () => {
      mockState.currentTab = 'channels';
      mockState.currentChannel = 'ch1';
      mockState.channelMessages = [];
      mockState.channels = [{
        id: 'ch1',
        name: 'general',
        members: [],
        lastMessage: '',
        lastSender: '',
        lastTimestamp: '',
        newMessageCount: 0,
        isDM: false,
      }];

      const { appendChannelMessage } = await import('../../stores/channel-actions');
      appendChannelMessage('ch1', {
        sender: 'lingxi',
        timestamp: '2026-05-07 17:01:00',
        body: 'hidden reply',
      }, { markRead: false });

      expect((mockState.channels as Array<{ newMessageCount: number }>)[0].newMessageCount).toBe(1);
      expect(mockFetch).not.toHaveBeenCalledWith('/api/channels/ch1/read', expect.anything());
    });

    it('reloads the active channel when a message-less event marked its cache dirty', async () => {
      vi.stubGlobal('window', { t: (key: string) => key });
      mockState.currentTab = 'channels';
      mockState.currentChannel = 'ch1';
      mockState.channelMessages = [
        { sender: 'testuser', timestamp: '2026-05-07 17:00:00', body: 'old' },
      ];
      mockState.channelMessageCache = {
        ch1: mockState.channelMessages,
      };
      mockState.channels = [{
        id: 'ch1',
        name: 'general',
        members: ['lingxi', 'yui'],
        lastMessage: 'old',
        lastSender: 'testuser',
        lastTimestamp: '2026-05-07 17:00:00',
        newMessageCount: 0,
        isDM: false,
      }];
      mockFetch
        .mockResolvedValueOnce({
          ok: true,
          json: async () => ({
            name: 'general',
            members: ['lingxi', 'yui'],
            messages: [
              { sender: 'testuser', timestamp: '2026-05-07 17:00:00', body: 'old' },
              { sender: 'lingxi', timestamp: '2026-05-07 17:01:00', body: 'reloaded reply' },
            ],
          }),
        } as Response)
        .mockResolvedValue({
          ok: true,
          json: async () => ({ activities: [] }),
        } as Response);

      const { markChannelMessagesDirty, hydrateCurrentChannelIfNeeded } = await import('../../stores/channel-actions');
      markChannelMessagesDirty('ch1');
      expect((mockState.channelMessageCacheDirty as any).ch1).toBe(true);

      await hydrateCurrentChannelIfNeeded();

      expect(mockFetch).toHaveBeenCalledWith('/api/channels/ch1');
      expect(mockState.channelMessages).toEqual([
        { sender: 'testuser', timestamp: '2026-05-07 17:00:00', body: 'old' },
        { sender: 'lingxi', timestamp: '2026-05-07 17:01:00', body: 'reloaded reply' },
      ]);
      expect((mockState.channelMessageCacheDirty as any).ch1).toBe(false);
    });
  });

  describe('createChannel', () => {
    it('reads backend JSON errors instead of losing them to the fetch wrapper', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 404,
        json: async () => ({
          code: 'CHANNEL_AGENT_NOT_FOUND',
          error: 'Agent not found: ghost',
        }),
      } as Response);

      const { createChannel } = await import('../../stores/channel-actions');

      await expect(createChannel('mixed', ['alice', 'ghost'])).rejects.toThrow('Agent not found: ghost');
      expect(mockFetch).toHaveBeenCalledWith('/api/channels', expect.objectContaining({
        method: 'POST',
        throwOnHttpError: false,
      }));
    });
  });

  describe('toggleChannelsEnabled', () => {
    it('未知频道开关状态时不猜测下一值', async () => {
      mockState.channelsEnabled = undefined;

      const { toggleChannelsEnabled } = await import('../../stores/channel-actions');
      const result = await toggleChannelsEnabled();

      expect(result).toBeUndefined();
      expect(mockFetch).not.toHaveBeenCalled();
      expect(setStateCalls).toEqual([]);
    });

    it('切换开关状态', async () => {
      mockState.channelsEnabled = true;

      mockFetch.mockResolvedValue({
        ok: true,
        json: async () => ({ channels: [] }),
      } as Response);

      const { toggleChannelsEnabled } = await import('../../stores/channel-actions');
      const result = await toggleChannelsEnabled();

      expect(result).toBe(false); // toggled from true to false
      // 状态通过后端 /api/channels/toggle 持久化，不再用 localStorage
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/channels/toggle'),
        expect.objectContaining({ method: 'POST' }),
      );
    });

    it('打开频道开关时先启用后端，再加载频道列表', async () => {
      mockState.channelsEnabled = false;
      mockFetch.mockImplementation(async (url: string) => {
        if (url === '/api/channels/toggle') {
          return {
            ok: true,
            json: async () => ({ ok: true, enabled: true }),
          } as Response;
        }
        if (url === '/api/channels') {
          return {
            ok: true,
            json: async () => ({ channels: [{ id: 'ch1', name: 'general', newMessageCount: 0 }] }),
          } as Response;
        }
        if (url === '/api/dm') {
          return {
            ok: true,
            json: async () => ({ dms: [] }),
          } as Response;
        }
        throw new Error(`unexpected fetch: ${url}`);
      });

      const { toggleChannelsEnabled } = await import('../../stores/channel-actions');
      const result = await toggleChannelsEnabled();

      expect(result).toBe(true);
      expect(mockFetch.mock.calls.map(([url]) => url)).toEqual([
        '/api/channels/toggle',
        '/api/channels',
        '/api/dm',
      ]);
      expect(mockState.channelsEnabled).toBe(true);
      expect(mockState.channels).toEqual([
        expect.objectContaining({ id: 'ch1', isDM: false }),
      ]);
    });
  });
  describe('请求失败与切换保护', () => {
    const response = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });
    const deferred = () => {
      let resolve!: (response: Response) => void;
      const promise = new Promise<Response>(done => { resolve = done; });
      return { promise, resolve };
    };

    it('列表失败保留旧目录与未读数，并明确记录错误', async () => {
      const old = [{ id: 'old', name: 'existing', newMessageCount: 7 }];
      mockState.channels = old;
      mockState.channelTotalUnread = 7;
      mockFetch.mockResolvedValueOnce(response({ error: 'unavailable' }, 503))
        .mockResolvedValueOnce(response({ dms: [] }));
      const { loadChannels } = await import('../../stores/channel-actions');
      await loadChannels();
      expect(mockState.channels).toBe(old);
      expect(mockState.channelTotalUnread).toBe(7);
      expect(mockState.channelListError).toContain('503');
      expect(mockState.channelListLoading).toBe(false);
    });

    it('缺少目录字段的响应不能伪装成空列表', async () => {
      mockState.channels = [{ id: 'old' }];
      mockFetch.mockResolvedValueOnce(response({ channels: [] })).mockResolvedValueOnce(response({}));
      const { loadChannels } = await import('../../stores/channel-actions');
      await loadChannels();
      expect(mockState.channels).toEqual([{ id: 'old' }]);
      expect(mockState.channelListError).toContain('dms');
    });

    it.each(['currentAgentId', 'serverPort'])('切换%s后迟到的目录不覆盖新目录', async field => {
      const pending = deferred();
      mockFetch.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(response({ dms: [] }));
      const { loadChannels } = await import('../../stores/channel-actions');
      const loading = loadChannels();
      mockState[field] = field === 'serverPort' ? '3211' : 'new-selection';
      mockState.channels = [{ id: 'new' }];
      pending.resolve(response({ channels: [{ id: 'old' }] }));
      await loading;
      expect(mockState.channels).toEqual([{ id: 'new' }]);
      expect(mockState.channelListError).toBeNull();
    });

    it('两次目录读取乱序返回时保留最新请求', async () => {
      const pending = deferred();
      mockFetch.mockReturnValueOnce(pending.promise)
        .mockResolvedValueOnce(response({ dms: [] }))
        .mockResolvedValueOnce(response({ channels: [{ id: 'new' }] }))
        .mockResolvedValueOnce(response({ dms: [] }));
      const { loadChannels } = await import('../../stores/channel-actions');
      const old = loadChannels();
      await loadChannels();
      pending.resolve(response({ channels: [{ id: 'old' }] }));
      await old;
      expect(mockState.channels).toEqual([expect.objectContaining({ id: 'new' })]);
    });

    it('历史读取失败保留同会话缓存，初次失败也留下错误而不是空历史', async () => {
      vi.stubGlobal('window', { t: (key: string) => key });
      const cached = [{ sender: 'hana', timestamp: '2026-10-16T00:00:00Z', body: 'cached' }];
      mockState.channelMessageCache = { ch1: cached };
      mockFetch.mockRejectedValueOnce(new Error('offline'));
      const { openChannel } = await import('../../stores/channel-actions');
      await openChannel('ch1');
      expect(mockState.channelMessages).toBe(cached);
      expect(mockState.channelHistoryError).toBe('offline');
      expect(mockState.channelHistoryLoading).toBe(false);
      expect(mockState.channelSettingsError).toBe('offline');
      mockFetch.mockRejectedValueOnce(new Error('not loaded'));
      await openChannel('ch2');
      expect(mockState.channelHistoryError).toBe('not loaded');
      expect((mockState.channelMessageCache as any).ch2).toBeUndefined();
    });

    it('旧频道历史返回后不覆盖新选择，也不继续读取旧频道设置', async () => {
      vi.stubGlobal('window', { t: (key: string) => key });
      const pending = deferred();
      mockFetch.mockReturnValueOnce(pending.promise);
      const { openChannel } = await import('../../stores/channel-actions');
      const loading = openChannel('ch1');
      mockState.currentChannel = 'ch2';
      mockState.channelMessages = [{ sender: 'new', timestamp: 'now', body: 'new selection' }];
      pending.resolve(response({ name: 'old', members: [], messages: [] }));
      await loading;
      expect(mockState.currentChannel).toBe('ch2');
      expect(mockState.channelMessages).toEqual([{ sender: 'new', timestamp: 'now', body: 'new selection' }]);
      expect(mockFetch).toHaveBeenCalledTimes(1);
    });

    it('设置读取失败不会覆盖真实旧值为默认设置', async () => {
      mockState.currentChannel = 'ch1';
      mockState.channelAgentPhoneToolMode = 'write';
      mockState.channelAgentGuardLimit = 8;
      mockFetch.mockRejectedValueOnce(new Error('settings offline'));
      const { loadConversationAgentPhoneSettings } = await import('../../stores/channel-actions');
      await loadConversationAgentPhoneSettings('ch1');
      expect(mockState.channelAgentPhoneToolMode).toBe('write');
      expect(mockState.channelAgentGuardLimit).toBe(8);
      expect(mockState.channelSettingsError).toBe('settings offline');
    });

    it('设置保存迟到时不覆盖另一个频道的值', async () => {
      mockState.currentChannel = 'ch1';
      const pending = deferred();
      mockFetch.mockReturnValueOnce(pending.promise);
      const { saveConversationAgentPhoneSettings } = await import('../../stores/channel-actions');
      const saving = saveConversationAgentPhoneSettings({ mode: 'write' });
      mockState.currentChannel = 'ch2';
      pending.resolve(response({ mode: 'write' }));
      await saving;
      expect(mockState.channelAgentPhoneToolMode).toBe('read_only');
    });

    it('缺少真实活动身份的响应不能清掉已有动态', async () => {
      const previous = { ch1: { hana: [{ agentId: 'hana', summary: 'existing' }] } };
      mockState.channelAgentActivities = previous;
      mockFetch.mockResolvedValueOnce(response({ activities: [{ timestamp: 'now', state: 'idle', summary: 'missing actor' }] }));
      const { loadConversationAgentActivities } = await import('../../stores/channel-actions');
      await loadConversationAgentActivities('ch1');
      expect(mockState.channelAgentActivities).toBe(previous);
    });

    it('私聊即使绕过输入框调用也不会发送用户消息', async () => {
      mockState.currentChannel = 'dm:peer';
      const { sendChannelMessage } = await import('../../stores/channel-actions');
      await expect(sendChannelMessage('do not send')).rejects.toThrow(/read only/);
      expect(mockFetch).not.toHaveBeenCalled();
    });

    it('频道发送失败交回调用方，不能添加假消息', async () => {
      mockState.currentChannel = 'ch1';
      mockFetch.mockRejectedValueOnce(new Error('send offline'));
      const { sendChannelMessage } = await import('../../stores/channel-actions');
      await expect(sendChannelMessage('keep text')).rejects.toThrow('send offline');
      expect(mockState.channelMessages).toEqual([]);
    });

    it('删除旧频道的响应不能清空后来选择的频道', async () => {
      mockState.currentChannel = 'ch1';
      const pending = deferred();
      mockFetch.mockReturnValueOnce(pending.promise)
        .mockResolvedValueOnce(response({ channels: [{ id: 'ch2' }] }))
        .mockResolvedValueOnce(response({ dms: [] }));
      const { deleteChannel } = await import('../../stores/channel-actions');
      const deleting = deleteChannel('ch1');
      mockState.currentChannel = 'ch2';
      pending.resolve(response({ ok: true }));
      await deleting;
      expect(mockState.currentChannel).toBe('ch2');
    });
  });

  describe('用户动作重试身份', () => {
    it('未确认的同一消息复用requestId，成功后相同正文是新动作', async () => {
      mockState.currentChannel = 'retry-channel';
      mockFetch.mockRejectedValueOnce(new Error('timeout'))
        .mockResolvedValueOnce(new Response(JSON.stringify({ ok: true, timestamp: '2026-10-16T01:00:00Z' })))
        .mockResolvedValue(new Response(JSON.stringify({ ok: true, timestamp: '2026-10-16T01:01:00Z' })));
      const { sendChannelMessage } = await import('../../stores/channel-actions');
      await expect(sendChannelMessage('retry-message')).rejects.toThrow('timeout');
      await sendChannelMessage('retry-message');
      await sendChannelMessage('retry-message');
      const sends = mockFetch.mock.calls.filter(([path]) => path.endsWith('/messages'));
      const ids = sends.map(([, options]) => JSON.parse(String(options?.body)).requestId);
      expect(ids[0]).toMatch(/^channel-message:/);
      expect(ids[1]).toBe(ids[0]);
      expect(ids[2]).not.toBe(ids[0]);
    });

    it('不同正文和不同目标必须分配新requestId', async () => {
      mockState.currentChannel = 'retry-a';
      mockFetch.mockRejectedValue(new Error('timeout'));
      const { sendChannelMessage } = await import('../../stores/channel-actions');
      await expect(sendChannelMessage('one')).rejects.toThrow();
      await expect(sendChannelMessage('two')).rejects.toThrow();
      mockState.currentChannel = 'retry-b';
      await expect(sendChannelMessage('two')).rejects.toThrow();
      const ids = mockFetch.mock.calls.map(([, options]) => JSON.parse(String(options?.body)).requestId);
      expect(new Set(ids).size).toBe(3);
    });

    it('创建超时后同一输入重试复用requestId，成员变化则是新动作', async () => {
      mockFetch.mockRejectedValue(new Error('create timeout'));
      const { createChannel } = await import('../../stores/channel-actions');
      await expect(createChannel('retry-create', ['a', 'b'])).rejects.toThrow();
      await expect(createChannel('retry-create', ['a', 'b'])).rejects.toThrow();
      await expect(createChannel('retry-create', ['a', 'c'])).rejects.toThrow();
      const ids = mockFetch.mock.calls.map(([, options]) => JSON.parse(String(options?.body)).requestId);
      expect(ids[0]).toMatch(/^channel-create:/);
      expect(ids[1]).toBe(ids[0]);
      expect(ids[2]).not.toBe(ids[0]);
    });
  });

});
