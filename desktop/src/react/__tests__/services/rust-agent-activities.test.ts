import { afterEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { captureSessionConnection } from '../../services/session-connection-owner';
import { refreshRustAgentActivities } from '../../services/rust-agent-activities';
import type { ServerConnection } from '../../services/server-connection';
import type { AgentActivityEntry } from '../../stores/agent-activity-slice';

const connection: ServerConnection = {
  connectionId: 'activities-a', kind: 'custom_remote', serverId: 'a', serverNodeKind: 'lingxi-service',
  studioId: 'studio', label: '活动服务', baseUrl: 'http://127.0.0.1:3210', wsUrl: 'ws://127.0.0.1:3210',
  token: 'activities-token', authState: 'paired', trustState: 'tunnel', credentialKind: 'device_credential', capabilities: [],
};
function select(source = connection) {
  useStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source });
  return captureSessionConnection()!;
}
function entry(patch: Partial<AgentActivityEntry> = {}): AgentActivityEntry {
  return { id: 'workflow-1', kind: 'workflow', status: 'running', sessionId: 's1', sessionPath: 'native:s1', agentId: null,
    agentName: null, summary: null, childSessionPath: null, startedAt: 100, finishedAt: null, ...patch };
}
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe('Rust 普通会话工作流活动快照', () => {
  it('从正式会话接口恢复持久活动，同时保留其他会话', async () => {
    const owner = select(); const other = entry({ id: 'other', sessionId: 's2', sessionPath: 'native:s2' });
    const externalActivities = ['subagent', 'cron', 'heartbeat'].map((kind, index) => entry({ id: `external-${index}`, kind: kind as AgentActivityEntry['kind'] }));
    useStore.setState({ agentActivitiesBySession: { s1: externalActivities, s2: [other] } });
    const fetchImpl = vi.fn(async () => ({ ok: true, json: async () => ({ entries: [entry({ status: 'failed', finishedAt: 200 })] }) }));
    vi.stubGlobal('fetch', fetchImpl);
    await refreshRustAgentActivities(owner, 's1');
    expect(fetchImpl).toHaveBeenCalledWith('http://127.0.0.1:3210/lingxi/v1/sessions/s1/agent-activities', expect.objectContaining({ headers: { Authorization: 'Bearer activities-token' } }));
    expect(useStore.getState().agentActivitiesBySession.s1[0]).toMatchObject({ status: 'failed', finishedAt: 200 });
    expect(useStore.getState().agentActivitiesBySession.s2).toEqual([other]);
    expect(useStore.getState().agentActivitiesBySession.s1).toEqual(expect.arrayContaining(externalActivities));
  });

  it('迟到的旧快照不能回滚实时完成结果，也不能用损坏输入清空页面', async () => {
    const owner = select();
    useStore.setState({ agentActivitiesBySession: { s1: [entry()] } });
    let deliver!: (data: unknown) => void;
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, json: () => new Promise(resolve => { deliver = resolve; }) })));
    const pending = refreshRustAgentActivities(owner, 's1');
    await vi.waitFor(() => expect(deliver).toBeTypeOf('function'));
    useStore.getState().upsertAgentActivity(entry({ status: 'done', finishedAt: 300 }));
    deliver({ entries: [entry()] }); await pending;
    expect(useStore.getState().agentActivitiesBySession.s1[0].status).toBe('done');
    const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, json: async () => ({ entries: [entry({ sessionId: 'foreign' })] }) })));
    await refreshRustAgentActivities(owner, 's1');
    expect(warning).toHaveBeenCalled();
    expect(useStore.getState().agentActivitiesBySession.s1[0].status).toBe('done');
  });

  it('换服务清除旧活动，并拒绝跨连接迟到的响应', async () => {
    const owner = select(); useStore.setState({ agentActivitiesBySession: { s1: [entry()] } });
    let deliver!: (data: unknown) => void;
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, json: () => new Promise(resolve => { deliver = resolve; }) })));
    const pending = refreshRustAgentActivities(owner, 's1');
    await vi.waitFor(() => expect(deliver).toBeTypeOf('function'));
    select({ ...connection, connectionId: 'activities-b', token: 'replacement' });
    expect(useStore.getState().agentActivitiesBySession).toEqual({});
    deliver({ entries: [entry()] }); await pending;
    expect(useStore.getState().agentActivitiesBySession).toEqual({});
  });
});
