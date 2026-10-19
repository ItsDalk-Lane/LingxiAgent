import { beforeEach, describe, expect, it, vi } from 'vitest';

type MockState = Record<string, unknown>;

const mockState: MockState = {};

const mockLingxiFetch = vi.fn();
const mockApplyAgentIdentity = vi.fn(async () => {});
const mockLoadAgents = vi.fn(async () => {});
const mockLoadMessages = vi.fn(async () => {});
const mockLoadSessions = vi.fn(async () => {});
const mockSwitchSession = vi.fn(async () => {});
const mockLoadModels = vi.fn(async () => {});
const mockActivateWorkspaceDesk = vi.fn(async () => {});
const mockLoadChannels = vi.fn(async () => {});
const mockApplyChatLayout = vi.fn();
const mockApplyEditorTypography = vi.fn();

vi.mock('../../stores', () => ({
  useStore: {
    getState: () => mockState,
    setState: (patch: MockState | ((s: MockState) => MockState)) => {
      const next = typeof patch === 'function' ? patch(mockState) : patch;
      Object.assign(mockState, next);
    },
  },
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: mockLingxiFetch,
}));

vi.mock('../../stores/agent-actions', () => ({
  applyAgentIdentity: mockApplyAgentIdentity,
  loadAgents: mockLoadAgents,
}));

vi.mock('../../stores/session-actions', () => ({
  loadMessages: mockLoadMessages,
  loadSessions: mockLoadSessions,
  switchSession: mockSwitchSession,
}));

vi.mock('../../utils/ui-helpers', () => ({
  loadModels: mockLoadModels,
}));

vi.mock('../../stores/desk-actions', () => ({
  activateWorkspaceDesk: mockActivateWorkspaceDesk,
}));

vi.mock('../../stores/channel-actions', () => ({
  loadChannels: mockLoadChannels,
}));

vi.mock('../../chat/layout', () => ({
  applyChatLayout: mockApplyChatLayout,
}));

vi.mock('../../editor/typography', () => ({
  applyEditorTypography: mockApplyEditorTypography,
}));

function jsonResponse(body: unknown): Response {
  return { json: async () => body } as unknown as Response;
}

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe('handleAppEvent', () => {
  it('预算提醒与工作流进度只更新有归属的数据，过期连接不生效', async () => {
    const addToast = vi.fn(); const upsertAgentActivity = vi.fn();
    const entry = { id: 'workflow-1', kind: 'workflow', sessionId: 's1', sessionPath: 'native:s1', status: 'running' };
    Object.assign(mockState, { addToast, upsertAgentActivity, agentActivitiesBySession: { s1: [entry] } });
    const { handleAppEvent } = await import('../../services/app-event-actions');
    handleAppEvent('goal_over_budget', { sessionId: 's1', name: '整理', kind: 'tokens' }, { isCurrent: () => false });
    expect(addToast).not.toHaveBeenCalled();
    handleAppEvent('goal_over_budget', { sessionId: 's1', name: '整理', kind: 'tokens' });
    expect(addToast).toHaveBeenCalledWith(expect.stringContaining('整理'), 'warning', 8_000, expect.any(Object));
    handleAppEvent('workflow_progress', { taskId: 'workflow-1', title: '无归属不得更新' });
    handleAppEvent('workflow_progress', { sessionId: 's1', sessionPath: 'native:other', taskId: 'workflow-1', title: '错配不得更新' });
    expect(upsertAgentActivity).not.toHaveBeenCalled();
    handleAppEvent('workflow_progress', { sessionId: 's1', taskId: 'workflow-1', title: '读取资料' });
    expect(upsertAgentActivity).toHaveBeenCalledWith({ ...entry, phaseLabel: '读取资料' });
    handleAppEvent('agent_activity', { entry: { ...entry, status: 'done' } });
    expect(upsertAgentActivity).toHaveBeenLastCalledWith({ ...entry, status: 'done' });
  });
  beforeEach(() => {
    Object.keys(mockState).forEach(k => delete mockState[k]);
    mockLingxiFetch.mockReset();
    mockApplyAgentIdentity.mockReset();
    mockLoadAgents.mockReset();
    mockLoadSessions.mockReset();
    mockSwitchSession.mockReset();
    mockLoadModels.mockReset();
    mockActivateWorkspaceDesk.mockReset();
    mockLoadChannels.mockReset();
    mockApplyChatLayout.mockReset();
    mockApplyEditorTypography.mockReset();
    vi.resetModules();

    (globalThis as Record<string, unknown>).window = {
      setTheme: vi.fn(),
      setSerifFont: vi.fn(),
      setPaperTexture: vi.fn(),
      dispatchEvent: vi.fn(),
    };
    (globalThis as Record<string, unknown>).i18n = {
      locale: 'zh-CN',
      defaultName: 'Hanako',
      load: vi.fn(async (locale: string) => {
        (globalThis as any).i18n.locale = locale;
      }),
    };
    (globalThis as Record<string, unknown>).WebSocket = { OPEN: 1 };
  });

  it('Bridge 状态和真实消息按当前连接消费，过期身份不触发页面或设置窗口', async () => {
    const reload = vi.fn(); const incoming = vi.fn(); const relay = vi.fn();
    Object.assign(mockState, { triggerBridgeReload: reload, addBridgeMessage: incoming, serverPort: 3210, serverToken: 'owner', serverNodeKind: 'lingxi-service' });
    (window as any).platform = { settingsChanged: relay };
    const { handleAppEvent } = await import('../../services/app-event-actions');
    handleAppEvent('bridge_status', { agentId: 'hana' }, { source: 'server', isCurrent: () => false });
    expect(reload).not.toHaveBeenCalled();
    handleAppEvent('bridge_status', { agentId: 'hana' }, { source: 'server', isCurrent: () => true });
    expect(reload).toHaveBeenCalledOnce(); expect(relay).toHaveBeenCalledWith('bridge-status-changed', expect.objectContaining({ agentId: 'hana', connectionKey: expect.any(String) }));
    expect(JSON.stringify(relay.mock.calls)).not.toContain('owner"');
    const message = { agentId: 'hana', platform: 'qq', sessionKey: 'native-key', text: '真实入站' };
    handleAppEvent('bridge_message', { message }, { isCurrent: () => true });
    expect(incoming).toHaveBeenCalledWith(message);
    handleAppEvent('bridge_message', { message: { text: '没有真实身份' } });
    expect(incoming).toHaveBeenCalledOnce();
  });

  it('RC 解绑必须匹配同一真实会话映射，迟到的旧解绑不能清掉新连接', async () => {
    Object.assign(mockState, { sessions: [{ path: 'native:session', rcAttachment: null }] });
    const { handleAppEvent } = await import('../../services/app-event-actions');
    handleAppEvent('bridge_rc_attached', { sessionPath: 'native:session', sessionKey: 'new-key', platform: 'wechat', title: '真实联系人' });
    handleAppEvent('bridge_rc_detached', { sessionPath: 'native:session', sessionKey: 'old-key' });
    expect((mockState.sessions as any[])[0].rcAttachment).toMatchObject({ sessionKey: 'new-key', platform: 'wechat' });
    handleAppEvent('bridge_rc_detached', { sessionPath: 'native:session', sessionKey: 'new-key' });
    expect((mockState.sessions as any[])[0].rcAttachment).toBeNull();
  });

  it('activity_update 按真实活动ID合并，并保留原有会话定位字段', async () => {
    Object.assign(mockState, { activities: [{ id: 'trigger-1', type: 'cron', sessionId: 'activity-1', runId: 'run-1', status: 'running' }, { id: 'other' }] });
    const { handleAppEvent } = await import('../../services/app-event-actions');
    handleAppEvent('activity_update', { activity: { id: 'trigger-1', status: 'done', summary: '真实结果' } });
    expect(mockState.activities).toEqual([{ id: 'trigger-1', type: 'cron', sessionId: 'activity-1', runId: 'run-1', status: 'done', summary: '真实结果' }, { id: 'other' }]);
    expect(mockLingxiFetch).not.toHaveBeenCalled();
  });

  it('voice_transcription_update 按真实会话与文件就地挂转写，未知附件不虚构更新', async () => {
    Object.assign(mockState, {
      chatSessions: {
        'native:sess-1': {
          items: [
            { type: 'message', data: { id: 'm1', role: 'user', attachments: [{ fileId: 'sf_abc', filename: 'voice.mp3' }, { fileId: 'sf_other', filename: 'note.txt' }] } },
            { type: 'message', data: { id: 'm2', role: 'assistant', text: '无附件' } },
          ],
        },
      },
    });
    const { handleAppEvent } = await import('../../services/app-event-actions');
    const { readMessageLiveVersion } = await import('../../stores/message-live-version');
    expect(readMessageLiveVersion('native:sess-1')).toBe(0);
    const transcription = { status: 'ready', text: '装配线实录', providerId: 'asr' };
    handleAppEvent('voice_transcription_update', { sessionId: 'sess-1', fileId: 'sf_abc', transcription });
    const session = (mockState.chatSessions as any)['native:sess-1'];
    expect(session.items[0].data.attachments[0].transcription).toEqual(transcription);
    expect(session.items[0].data.attachments[1].transcription).toBeUndefined();
    expect(session.items[1].data.attachments).toBeUndefined();
    expect(readMessageLiveVersion('native:sess-1')).toBe(1);
    // 未知 fileId、未知会话、缺字段：一律不产生状态变化，也不推进 live version。
    const before = JSON.stringify(mockState.chatSessions);
    handleAppEvent('voice_transcription_update', { sessionId: 'sess-1', fileId: 'sf_missing', transcription });
    handleAppEvent('voice_transcription_update', { sessionId: 'sess-absent', fileId: 'sf_abc', transcription });
    handleAppEvent('voice_transcription_update', { sessionId: 'sess-1', fileId: 'sf_abc' });
    handleAppEvent('voice_transcription_update', { fileId: 'sf_abc', transcription });
    handleAppEvent('voice_transcription_update', { sessionId: 'sess-1', fileId: 'sf_abc', transcription: 'not-an-object' });
    expect(JSON.stringify(mockState.chatSessions)).toBe(before);
    expect(readMessageLiveVersion('native:sess-1')).toBe(1);
  });

  it('cron_job_done 刷新真实任务计数，旧连接结果和坏格式不能覆盖现值', async () => {
    Object.assign(mockState, { automationCount: 7 });
    const { handleAppEvent } = await import('../../services/app-event-actions');
    let resolve!: (response: Response) => void;
    mockLingxiFetch.mockReturnValueOnce(new Promise<Response>(done => { resolve = done; }));
    let current = true;
    const pending = handleAppEvent('cron_job_done', { jobId: 'job-1' }, { isCurrent: () => current });
    current = false;
    resolve(jsonResponse({ jobs: [] }));
    await pending;
    expect(mockState.automationCount).toBe(7);
    mockLingxiFetch.mockResolvedValueOnce(jsonResponse({ jobs: [{ id: 'job-1' }, { id: 'job-2' }] }));
    await handleAppEvent('cron_job_done', { jobId: 'job-1' });
    expect(mockState.automationCount).toBe(2);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    mockLingxiFetch.mockResolvedValueOnce(jsonResponse({ jobs: null }));
    await handleAppEvent('cron_job_done', { jobId: 'job-1' });
    expect(mockState.automationCount).toBe(2);
    expect(warn).toHaveBeenCalledOnce();
    warn.mockRestore();
  });

  it('agent-switched applies agent identity, reloads dependent data, and resets agent-scoped UI state', async () => {
    mockLingxiFetch
      // tenets 初拉（agent-switched 现在会先刷新用户原则待审列表）
      .mockResolvedValueOnce(jsonResponse({ tenets: [] }))
      .mockResolvedValueOnce(jsonResponse({ jobs: [{ id: 'job-1' }] }));
    Object.assign(mockState, {
      currentChannel: { id: 'old' },
      channelMessages: [{ id: 'm1' }],
      thinkingLevel: 'high',
      activities: [{ id: 'a1' }],
    });

    const { handleAppEvent } = await import('../../services/app-event-actions');
    handleAppEvent('agent-switched', {
      agentName: 'Hana',
      agentId: 'agent-a',
      sessionPath: '/sessions/agent-a.jsonl',
      cwd: '/agent-cwd',
      homeFolder: '/agent-home',
      workspaceFolders: ['/reference'],
      cwdHistory: ['/agent-cwd', '/recent'],
      memoryMasterEnabled: false,
    });
    await flushPromises();

    expect(mockApplyAgentIdentity).toHaveBeenCalledWith({
      agentName: 'Hana',
      agentId: 'agent-a',
    });
    expect(mockLoadSessions).toHaveBeenCalledTimes(1);
    expect(mockSwitchSession).toHaveBeenCalledWith('/sessions/agent-a.jsonl');
    expect(mockLoadChannels).toHaveBeenCalledTimes(1);
    expect(mockLoadModels).toHaveBeenCalledTimes(1);
    expect(mockState.currentChannel).toBeNull();
    expect(mockState.channelMessages).toEqual([]);
    expect(mockState.channelMembers).toEqual([]);
    expect(mockState.channelTotalUnread).toBe(0);
    expect(mockState.channelHeaderName).toBe('');
    expect(mockState.channelHeaderMembersText).toBe('');
    expect(mockState.channelInfoName).toBe('');
    expect(mockState.channelIsDM).toBe(false);
    expect(mockState.thinkingLevel).toBe('high');
    expect(mockState.activities).toEqual([]);
    expect(mockState.homeFolder).toBe('/agent-home');
    expect(mockState.workspaceFolders).toEqual(['/reference']);
    expect(mockState.cwdHistory).toEqual(['/agent-cwd', '/recent']);
    expect(mockState.memoryMasterEnabled).toBe(false);
  });

  it('models-changed reloads models', async () => {
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('models-changed');

    expect(mockLoadModels).toHaveBeenCalledTimes(1);
  });

  it('forwards server models-changed through the existing settings window channel', async () => {
    const settingsChanged = vi.fn();
    (globalThis as any).window.platform = { settingsChanged };
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('models-changed', { reason: 'provider' }, { source: 'server' });

    expect(mockLoadModels).toHaveBeenCalledTimes(1);
    expect(settingsChanged).toHaveBeenCalledWith('models-changed', { reason: 'provider' });
  });

  it('emits a same-window models event for an already mounted settings modal', async () => {
    const { handleAppEvent } = await import('../../services/app-event-actions');
    const dispatchEvent = vi.mocked(globalThis.window.dispatchEvent);

    handleAppEvent('models-changed', { reason: 'provider' }, { source: 'server' });

    expect(dispatchEvent).toHaveBeenCalledTimes(1);
    const event = dispatchEvent.mock.calls[0][0] as CustomEvent;
    expect(event.type).toBe('hana-models-changed');
    expect(event.detail).toEqual({ reason: 'provider' });
  });

  it('models-changed requests context usage through the injected callback', async () => {
    Object.assign(mockState, { currentSessionPath: '/session/a.jsonl' });
    const requestContextUsage = vi.fn();
    const { configureAppEventActions, handleAppEvent } = await import('../../services/app-event-actions');

    configureAppEventActions({ requestContextUsage });
    handleAppEvent('models-changed');

    expect(mockLoadModels).toHaveBeenCalledTimes(1);
    expect(requestContextUsage).toHaveBeenCalledWith('/session/a.jsonl');
  });

  it('skills-changed increments the skill catalog revision and emits a browser event', async () => {
    Object.assign(mockState, { skillCatalogVersion: 2 });
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('skills-changed', { agentId: 'agent-a' });

    expect(mockState.skillCatalogVersion).toBe(3);
    expect((globalThis as any).window.dispatchEvent).toHaveBeenCalledTimes(1);
    const event = ((globalThis as any).window.dispatchEvent as any).mock.calls[0][0] as CustomEvent;
    expect(event.type).toBe('hana-skills-changed');
    expect(event.detail).toEqual({ agentId: 'agent-a' });
  });

  it('agent-updated for a non-current agent refreshes the agent list without applying identity', async () => {
    Object.assign(mockState, { currentAgentId: 'agent-a', agentName: 'Hana' });
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('agent-updated', {
      agentId: 'agent-b',
      agentName: 'Other Agent',
      yuan: 'other-yuan',
    });

    expect(mockApplyAgentIdentity).not.toHaveBeenCalled();
    expect(mockLoadAgents).toHaveBeenCalledTimes(1);
  });

  it('agent-updated for the current agent keeps refreshing identity and avatar state', async () => {
    Object.assign(mockState, { currentAgentId: 'agent-a', agentName: 'Hana' });
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('agent-updated', {
      agentId: 'agent-a',
      agentName: 'Hana Prime',
      yuan: 'muse',
    });

    expect(mockApplyAgentIdentity).toHaveBeenCalledWith({
      agentName: 'Hana Prime',
      agentId: 'agent-a',
      yuan: 'muse',
      ui: { settings: false },
    });
    expect(mockLoadAgents).not.toHaveBeenCalled();
  });

  it('memory-master-changed updates the current agent gate and cached agent row', async () => {
    Object.assign(mockState, {
      currentAgentId: 'agent-a',
      memoryMasterEnabled: true,
      agents: [
        { id: 'agent-a', name: 'Hana', memoryMasterEnabled: true },
        { id: 'agent-b', name: 'Other', memoryMasterEnabled: true },
      ],
    });
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('memory-master-changed', { agentId: 'agent-a', enabled: false });

    expect(mockState.memoryMasterEnabled).toBe(false);
    expect((mockState.agents as any[]).find(a => a.id === 'agent-a')?.memoryMasterEnabled).toBe(false);
    expect((mockState.agents as any[]).find(a => a.id === 'agent-b')?.memoryMasterEnabled).toBe(true);
  });

  it('agent-switched reads the next agent memory gate from config', async () => {
    mockLingxiFetch
      // tenets 初拉（agent-switched 现在会先刷新用户原则待审列表）
      .mockResolvedValueOnce(jsonResponse({ tenets: [] }))
      .mockResolvedValueOnce(jsonResponse({ jobs: [] }));
    Object.assign(mockState, { currentAgentId: 'agent-a', memoryMasterEnabled: true });
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('agent-switched', {
      agentName: 'Other',
      agentId: 'agent-b',
      sessionPath: '/sessions/agent-b.jsonl',
      cwd: '/agent-home',
      homeFolder: '/agent-home',
      cwdHistory: ['/recent'],
      memoryMasterEnabled: false,
    });
    await vi.waitFor(() => {
      expect(mockState.memoryMasterEnabled).toBe(false);
    });

    expect(mockLingxiFetch).not.toHaveBeenCalledWith('/api/config');
  });

  it('theme-changed applies the selected theme', async () => {
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('theme-changed', { theme: 'moon' });

    expect((globalThis as any).window.setTheme).toHaveBeenCalledWith('moon');
  });

  it('editor-typography-changed applies editor typography settings', async () => {
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('editor-typography-changed', {
      editor: { markdown: { bodyFontSize: 17 } },
    });

    expect(mockApplyEditorTypography).toHaveBeenCalledWith({
      markdown: { bodyFontSize: 17 },
    });
  });

  it('chat-layout-changed applies chat layout settings', async () => {
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('chat-layout-changed', {
      chat: { contentWidth: 800 },
    });

    expect(mockApplyChatLayout).toHaveBeenCalledWith({
      contentWidth: 800,
    });
  });

  it('experiment-changed forwards the setting into the main-window browser event', async () => {
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('experiment-changed', {
      id: 'session.instant_simple_compaction',
      value: true,
    });

    expect((globalThis as any).window.dispatchEvent).toHaveBeenCalledTimes(1);
    const event = ((globalThis as any).window.dispatchEvent as any).mock.calls[0][0] as CustomEvent;
    expect(event.type).toBe('hana-settings');
    expect(event.detail).toEqual({
      type: 'experiment-changed',
      id: 'session.instant_simple_compaction',
      value: true,
    });
  });

  it('does not echo desktop IPC network proxy broadcasts back to the main process', async () => {
    const settingsChanged = vi.fn();
    (globalThis as any).window.platform = { settingsChanged };

    const { handleAppEvent } = await import('../../services/app-event-actions');
    (handleAppEvent as any)(
      'network-proxy-changed',
      { network_proxy: { mode: 'direct' } },
      { source: 'desktop-ipc' },
    );

    expect(settingsChanged).not.toHaveBeenCalled();
  });

  it('forwards server network proxy app events to the desktop shell once', async () => {
    const settingsChanged = vi.fn();
    (globalThis as any).window.platform = { settingsChanged };

    const { handleAppEvent } = await import('../../services/app-event-actions');
    (handleAppEvent as any)(
      'network-proxy-changed',
      { network_proxy: { mode: 'direct' } },
      { source: 'server' },
    );

    expect(settingsChanged).toHaveBeenCalledTimes(1);
    expect(settingsChanged).toHaveBeenCalledWith('network-proxy-changed', { network_proxy: { mode: 'direct' } });
  });

  it('agent-workspace-changed updates only the current agent workspace and activates the desk', async () => {
    Object.assign(mockState, {
      currentAgentId: 'agent-a',
      homeFolder: '/old-home',
      selectedFolder: '/old-home',
      workspaceFolders: ['/old-home/project'],
      cwdHistory: ['/old-home'],
      deskBasePath: '/old-home',
      pendingNewSession: true,
      currentSessionPath: null,
    });

    const { handleAppEvent } = await import('../../services/app-event-actions');
    handleAppEvent('agent-workspace-changed', {
      agentId: 'agent-a',
      homeFolder: '/new-home',
      cwdHistory: ['/new-home', '/recent'],
    });

    expect(mockState.homeFolder).toBe('/new-home');
    expect(mockState.selectedFolder).toBe('/new-home');
    expect(mockState.workspaceFolders).toEqual([]);
    expect(mockState.cwdHistory).toEqual(['/old-home']);
    expect(mockActivateWorkspaceDesk).toHaveBeenCalledWith('/new-home', { mountId: null });

    mockActivateWorkspaceDesk.mockClear();
    handleAppEvent('agent-workspace-changed', {
      agentId: 'agent-b',
      homeFolder: '/other-home',
      cwdHistory: ['/other-home'],
    });

    expect(mockState.homeFolder).toBe('/new-home');
    expect(mockState.selectedFolder).toBe('/new-home');
    expect(mockState.cwdHistory).toEqual(['/old-home']);
    expect(mockActivateWorkspaceDesk).not.toHaveBeenCalled();
  });

  it('agent-created reloads both agents and channels so the new DM appears immediately', async () => {
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('agent-created', { agentId: 'agent-new' });

    expect(mockLoadAgents).toHaveBeenCalledTimes(1);
    expect(mockLoadChannels).toHaveBeenCalledTimes(1);
  });

  it('agent-deleted reloads both agents and channels so the stale DM disappears immediately', async () => {
    Object.assign(mockState, { currentChannel: 'other-channel' });
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('agent-deleted', { agentId: 'agent-gone' });

    expect(mockLoadAgents).toHaveBeenCalledTimes(1);
    expect(mockLoadChannels).toHaveBeenCalledTimes(1);
  });

  it('agent-deleted clears current session state when the open DM belongs to the deleted agent', async () => {
    Object.assign(mockState, {
      currentChannel: 'dm:agent-gone',
      channelMessages: [{ sender: 'agent-gone', body: 'hi', timestamp: '2026-01-01' }],
      channelHeaderName: 'agent-gone',
      channelHeaderMembersText: '2 members',
      channelIsDM: true,
    });
    const { handleAppEvent } = await import('../../services/app-event-actions');

    handleAppEvent('agent-deleted', { agentId: 'agent-gone' });

    expect(mockState.currentChannel).toBeNull();
    expect(mockState.channelMessages).toEqual([]);
    expect(mockState.channelHeaderName).toBe('');
    expect(mockState.channelHeaderMembersText).toBe('');
    expect(mockState.channelIsDM).toBe(false);
  });
});


describe('协作决定仅刷新真实源会话',()=>{
 it('排队被实际受理后刷新可见源会话，忽略其他会话和过期连接',async()=>{
  const {handleAppEvent}=await import('../../services/app-event-actions');
  mockLoadMessages.mockClear();Object.assign(mockState,{currentSessionPath:'native:source',sessions:[]});
  await handleAppEvent('session_collab_decision',{sourceSessionId:'other',sessionId:'other'});
  await handleAppEvent('session_collab_decision',{sourceSessionId:'source',sessionId:'source'},{isCurrent:()=>false});
  expect(mockLoadMessages).not.toHaveBeenCalled();
  await handleAppEvent('session_collab_decision',{sourceSessionId:'source',sessionId:'source',result:{deliveryStatus:'accepted'}});
  expect(mockLoadMessages).toHaveBeenCalledWith('native:source');
 });
});
