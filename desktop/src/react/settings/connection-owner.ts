import { useSettingsStore } from './store';
import { createRemoteResource } from './resource-state';
import { resolveServerConnection } from '../services/server-connection';

/** 与实际 fetch 使用完全相同的连接；计数锁存离开后又返回的变化。 */
function identity(): string {
  const connection = resolveServerConnection(useSettingsStore.getState());
  return connection ? JSON.stringify(connection, (_key, value: unknown) => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return value;
    const object = value as Record<string, unknown>;
    return Object.fromEntries(Object.keys(object).sort().map(key => [key, object[key]]));
  }) : '';
}
let currentKey = identity();
let revision = 0;
// 单一模块级观察者与设置 store 同寿命，变化在 React 下一次渲染前同步生效。
useSettingsStore.subscribe(() => {
  const next = identity();
  if (next !== currentKey) {
    currentKey = next; revision++;
    // 连接改变时同步撤销旧资料，不等页面 effect；同一实际连接重开不触发清理。
    useSettingsStore.setState({ settingsAgentsOwner: null, settingsDataOwner: null, settingsDataAgentId: null,
      settingsConfig: null, settingsConfigKey: null, settingsConfigStatus: 'idle', settingsConfigError: null,
      settingsSnapshot: createRemoteResource(), globalModelsConfig: null, runtimeModels: [],
      providersSummary: {}, agents: [], currentAgentId: null, settingsAgentId: null,
      userAvatarUrl: null, agentAvatarUrl: null, homeFolder: null,
      agentsError: null, runtimeModelsError: null, providersSummaryError: null,
    });
  }
});
export interface SettingsConnectionOwner { key: string; revision: number }
export function captureSettingsConnection(): SettingsConnectionOwner { return { key: identity(), revision }; }
export function ownsSettingsConnection(owner: SettingsConnectionOwner | null): boolean {
  return !!owner && owner.key === identity() && owner.revision === revision;
}
export function settingsConnectionRevision(): number { return revision; }
