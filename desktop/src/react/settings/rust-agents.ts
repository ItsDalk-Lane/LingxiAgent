import { captureSettingsConnection, ownsSettingsConnection } from './connection-owner';
import { lingxiFetchJson } from './api';
import { useSettingsStore, type Agent } from './store';

/** Rust助手列表是独立入口；读取失败直接显示，不以旧Node快照冒充当前数据。 */
export async function loadRustAgents(signal?: AbortSignal): Promise<void> {
  const owner = captureSettingsConnection();
  const data = await lingxiFetchJson<{ agents: Array<Agent & { isCurrent?: boolean }> }>('/api/agents', { signal });
  if (!Array.isArray(data.agents) || data.agents.some(agent => typeof agent.id !== 'string' || typeof agent.name !== 'string')) {
    throw new Error('助手列表返回格式不正确');
  }
  if (signal?.aborted || !ownsSettingsConnection(owner)) return;
  const state = useSettingsStore.getState();
  const current = data.agents.find(agent => agent.isCurrent)
    || data.agents.find(agent => agent.id === state.currentAgentId)
    || data.agents.find(agent => agent.isPrimary) || data.agents[0];
  const selected = data.agents.find(agent => agent.id === state.settingsAgentId) || current;
  state.set({ settingsAgentsOwner: owner, agents: data.agents, currentAgentId: current?.id ?? null, settingsAgentId: selected?.id ?? null });
}
