import { useStore } from '../stores';
import type { AgentActivityEntry } from '../stores/agent-activity-slice';
import type { AgentActivitySnapshot } from '../../../../contracts/generated/ts/lingxi-protocol';
import { ownsSessionConnection, sessionOwnerFetch, type SessionConnectionOwner } from './session-connection-owner';

/** 初次选择和重连从同一服务恢复后台活动；迟到快照不能盖掉实时更新。 */
export async function refreshRustAgentActivities(owner: SessionConnectionOwner, sessionId: string): Promise<void> {
  if (owner.connection.serverNodeKind !== 'lingxi-service' || !sessionId || !ownsSessionConnection(owner)) return;
  const path = `native:${sessionId}`;
  const beforeState = useStore.getState();
  const before = beforeState.agentActivitiesBySession[sessionId] ?? beforeState.agentActivitiesBySession[path] ?? [];
  const byId = new Map(before.map(entry => [entry.id, entry]));
  try {
    const response = await sessionOwnerFetch(owner, `/lingxi/v1/sessions/${encodeURIComponent(sessionId)}/agent-activities`);
    const data: AgentActivitySnapshot = await response.json();
    if (!ownsSessionConnection(owner)) return;
    if (!data || !Array.isArray(data.entries) || data.entries.length > 10_000
      || data.entries.some((entry: AgentActivityEntry) => !entry || typeof entry.id !== 'string' || !entry.id
        || entry.sessionId !== sessionId || entry.sessionPath !== path
        || !['subagent', 'workflow', 'workflow_agent', 'workflow_step', 'heartbeat', 'cron'].includes(entry.kind)
        || !['running', 'done', 'failed', 'aborted'].includes(entry.status))) {
      throw new Error('会话活动快照格式无效');
    }
    if (new Set(data.entries.map((entry: AgentActivityEntry) => entry.id)).size !== data.entries.length) throw new Error('会话活动快照编号重复');
    useStore.setState(state => {
      if (!ownsSessionConnection(owner)) return {};
      const current = state.agentActivitiesBySession[sessionId] ?? state.agentActivitiesBySession[path] ?? [];
      const merged = new Map<string, AgentActivityEntry>(data.entries.map((entry: AgentActivityEntry) => [entry.id, entry]));
      for (const entry of current) {
        // 该快照只负责工作流家族，不能清除其他来源维护的活动。
        if (!['workflow', 'workflow_agent', 'workflow_step'].includes(entry.kind)
          || byId.get(entry.id) !== entry) merged.set(entry.id, entry);
      }
      const next = { ...state.agentActivitiesBySession, [sessionId]: [...merged.values()] };
      delete next[path];
      return { agentActivitiesBySession: next };
    });
  } catch (error) {
    if (ownsSessionConnection(owner)) console.warn('[agent-activities] 活动读取失败，保留现有记录', error);
  }
}
