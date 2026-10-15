import { captureSettingsConnection, ownsSettingsConnection, type SettingsConnectionOwner } from './connection-owner';
/**
 * Settings shared actions — extracted from SettingsApp to avoid circular imports
 */
import { useSettingsStore } from './store';
import { lingxiFetch, lingxiFetchJson, lingxiUrl, settingsConnectionKey } from './api';
import { t } from './helpers';
import { errorWithCode, localizedReasonOrRaw } from '../errors/error-presenter';
import { normalizeSessionRouteError } from '../../../../shared/error-user-messages.ts';
import {
  createRemoteResource,
  failRemoteLoad,
  finishRemoteLoad,
  makeSettingsResourceKey,
  startRemoteLoad,
} from './resource-state';
import type { ProviderSummary, RuntimeModelInfo, SettingsSnapshot } from './store';

let _settingsConfigLoadVersion = 0;
let _settingsConfigAbortController: AbortController | null = null;
let _settingsSnapshotLoadVersion = 0;
let _settingsSnapshotAbortController: AbortController | null = null;
let _settingsModelsLoadVersion = 0;
let _settingsModelsAbortController: AbortController | null = null;

function isAbortError(err: unknown): boolean {
  return !!err && typeof err === 'object' && (err as { name?: string }).name === 'AbortError';
}

export async function loadAgents() {
  const owner = captureSettingsConnection();
  const store = useSettingsStore.getState();
  const connectionKey = settingsConnectionKey();
  try {
    const res = await lingxiFetch('/api/agents');
    const data = await res.json();
    if (!ownsSettingsConnection(owner) || settingsConnectionKey() !== connectionKey) return;
    if (data.error) throw new Error(data.error);
    if (!Array.isArray(data.agents)) throw new Error('助手列表响应缺少 agents');
    const agents = data.agents;
    let currentAgentId = store.currentAgentId;
    if (!currentAgentId || !agents.some((agent: any) => agent.id === currentAgentId)) {
      const primary = agents.find((a: any) => a.isPrimary) || agents[0];
      if (primary) currentAgentId = primary.id;
    }
    const currentAgent = agents.find((a: any) => a.id === currentAgentId);
    store.set({
      agents,
      settingsAgentsOwner: owner,
      agentsError: null,
      currentAgentId,
      agentYuan: currentAgent?.yuan || store.agentYuan,
      agentName: currentAgent?.name || store.agentName,
    });
  } catch (err) {
    console.error('[agents] load failed:', err);
    store.set({ agentsError: err instanceof Error ? err.message : String(err) });
  }
}

/**
 * 设置页的头像。user 头像与 agent 无关，直接取 health 的可用性标记；
 * agent 头像属于设置页当前选中的那个 agent —— 它未必是服务端正在聚焦的
 * agent，所以可用性从 agents 列表里那一条读，URL 也显式带上 agentId。
 */
/**
 * 供应商摘要（列表徽标/模型计数/凭证状态）。与其它 tab 数据一样由 initSettings 在
 * 连接就绪后统一加载——ProvidersTab 挂载时不得自行抢跑（子组件 effect 先于父组件
 * initSettings 执行，连接未就绪时 fetch 必败且静默吞掉后无重试）。
 */
export async function loadProvidersSummary() {
  const connectionKey = settingsConnectionKey();
  try {
    const data = await lingxiFetchJson<{ providers?: Record<string, ProviderSummary> }>('/api/providers/summary');
    if (settingsConnectionKey() !== connectionKey) return;
    if (!data.providers || typeof data.providers !== 'object' || Array.isArray(data.providers)) throw new Error('供应商摘要响应格式无效');
    useSettingsStore.setState({ providersSummary: data.providers, providersSummaryError: null });
  } catch (failure) {
    useSettingsStore.setState({ providersSummaryError: failure instanceof Error ? failure.message : String(failure) });
    throw failure;
  }
}

export async function loadAvatars() {
  const ts = Date.now();
  const store = useSettingsStore.getState();
  try {
    const res = await lingxiFetch('/api/health');
    const data = await res.json();
    const avatars = data.avatars || {};
    const agentId = store.getSettingsAgentId();
    const agent = agentId ? (store.agents || []).find((a: any) => a.id === agentId) : null;

    store.set({
      userAvatarUrl: avatars.user ? lingxiUrl(`/api/avatar/user?t=${ts}`) : null,
      agentAvatarUrl: agentId && agent?.hasAvatar
        ? lingxiUrl(`/api/avatar/agent?agentId=${encodeURIComponent(agentId)}&t=${ts}`)
        : null,
    });
  } catch {}
}

/** 设置窗口里所有模型选择器共享的运行时模型目录。 */
export async function loadSettingsModels(): Promise<void> {
  const owner = captureSettingsConnection();
  const myVersion = ++_settingsModelsLoadVersion;
  const connectionKey = settingsConnectionKey();
  _settingsModelsAbortController?.abort();
  const controller = new AbortController();
  _settingsModelsAbortController = controller;

  try {
    const data = await lingxiFetchJson<{ models?: RuntimeModelInfo[] }>('/api/models', {
      signal: controller.signal,
    });
    if (!ownsSettingsConnection(owner) || settingsConnectionKey() !== connectionKey) return;
    if (myVersion !== _settingsModelsLoadVersion || _settingsModelsAbortController !== controller) return;
    if (!Array.isArray(data.models)) throw new Error('模型目录响应缺少 models');
    const runtimeModels = Array.isArray(data.models)
      ? data.models.filter(model => (
          !!model
          && typeof model.id === 'string'
          && typeof model.provider === 'string'
        ))
      : [];
    useSettingsStore.getState().set({ runtimeModels, runtimeModelsError: null });
  } catch (err) {
    if (!ownsSettingsConnection(owner)) return;
    if (!isAbortError(err)) {
      console.error('[settings] models load failed:', err);
      useSettingsStore.setState({ runtimeModelsError: err instanceof Error ? err.message : String(err) });
    }
  } finally {
    if (_settingsModelsAbortController === controller) {
      _settingsModelsAbortController = null;
    }
  }
}

export async function loadSettingsConfig() {
  const owner = captureSettingsConnection();
  const store = useSettingsStore.getState();
  const myVersion = ++_settingsConfigLoadVersion;
  if (_settingsConfigAbortController) {
    _settingsConfigAbortController.abort();
  }
  const controller = new AbortController();
  _settingsConfigAbortController = controller;
  const agentId = store.getSettingsAgentId();
  const resourceKey = makeSettingsResourceKey('config', agentId, settingsConnectionKey());
  const keepSameOwnerData = store.settingsConfigKey === resourceKey;
  store.set({
    settingsConfigKey: resourceKey,
    settingsConfigStatus: 'loading',
    settingsConfigError: null,
    ...(keepSameOwnerData ? {} : {
      settingsConfig: null,
      globalModelsConfig: null,
      homeFolder: null,
    }),
  });
  if (!agentId || !resourceKey) {
    store.set({
      settingsConfigStatus: 'error',
      settingsConfigError: 'No settings agent selected',
      settingsConfig: null,
      globalModelsConfig: null,
      homeFolder: null,
    });
    return;
  }
  try {
    const agentBase = `/api/agents/${agentId}`;
    const [configRes, identityRes, agentsMdRes, publicAgentsMdRes, userProfileRes, globalModelsRes] =
      await Promise.all([
        lingxiFetch(`${agentBase}/config`, { signal: controller.signal }),
        lingxiFetch(`${agentBase}/identity`, { signal: controller.signal }),
        lingxiFetch(`${agentBase}/agents-md`, { signal: controller.signal }),
        lingxiFetch(`${agentBase}/public-agents-md`, { signal: controller.signal }),
        lingxiFetch('/api/user-profile', { signal: controller.signal }),
        lingxiFetch('/api/preferences/models', { signal: controller.signal }),
      ]);

    const config = await configRes.json();
    const globalModels = await globalModelsRes.json();
    const identityData = await identityRes.json();
    config._identity = identityData.content || '';
    const agentsMdData = await agentsMdRes.json();
    config._agents = agentsMdData.content || '';
    const publicAgentsMdData = await publicAgentsMdRes.json();
    config._publicAgents = publicAgentsMdData.content || '';
    const userProfileData = await userProfileRes.json();
    config._userProfile = userProfileData.content || '';
    config._experience = '';
    if (config.experience?.enabled === true) {
      const experienceRes = await lingxiFetch(`${agentBase}/experience`, { signal: controller.signal });
      const experienceData = await experienceRes.json();
      config._experience = experienceData.content || '';
    }
    if (!ownsSettingsConnection(owner)) return;
    if (resourceKey !== makeSettingsResourceKey('config', useSettingsStore.getState().getSettingsAgentId(), settingsConnectionKey())) return;
    if (myVersion !== _settingsConfigLoadVersion) return;
    if (_settingsConfigAbortController !== controller) return;
    const latest = useSettingsStore.getState();
    if (latest.settingsConfigKey !== resourceKey) return;

    store.set({
      settingsConfigKey: resourceKey,
      settingsConfigStatus: 'ready',
      settingsConfigError: null,
      settingsDataOwner: owner, settingsDataAgentId: agentId,
      settingsConfig: config,
      globalModelsConfig: globalModels,
      homeFolder: config.desk?.home_folder || null,
    });
  } catch (err) {
    if (isAbortError(err)) return;
    console.error('[settings] load failed:', err);
    const latest = useSettingsStore.getState();
    if (latest.settingsConfigKey === resourceKey && myVersion === _settingsConfigLoadVersion) {
      store.set({
        settingsConfigStatus: 'error',
        settingsConfigError: err instanceof Error ? err.message : String(err),
      });
    }
  } finally {
    if (_settingsConfigAbortController === controller) {
      _settingsConfigAbortController = null;
    }
  }
}

function configFromSnapshot(snapshot: SettingsSnapshot): Record<string, any> {
  return {
    ...(snapshot.config || {}),
    _identity: snapshot.identity || '',
    _agents: snapshot.agents || '',
    _publicAgents: snapshot.publicAgents || '',
    _userProfile: snapshot.userProfile || '',
    _experience: snapshot.experience || '',
  };
}

function applySettingsSnapshot(snapshot: SettingsSnapshot, resourceKey: string, requestId: number) {
  const latest = useSettingsStore.getState();
  if (latest.settingsSnapshot.key !== resourceKey || latest.settingsSnapshot.requestId !== requestId) return;
  const config = configFromSnapshot(snapshot);
  const configKey = makeSettingsResourceKey('config', snapshot.agentId, settingsConnectionKey());
  latest.set({
    settingsSnapshot: finishRemoteLoad(latest.settingsSnapshot, resourceKey, requestId, snapshot),
    settingsConfigKey: configKey,
    settingsConfigStatus: 'ready',
    settingsConfigError: null,
    settingsConfig: config,
    settingsDataOwner: captureSettingsConnection(), settingsDataAgentId: snapshot.agentId,
    globalModelsConfig: snapshot.globalModels || {},
    homeFolder: config.desk?.home_folder || null,
  });
}

export function updateSettingsSnapshot(mutator: (snapshot: SettingsSnapshot) => SettingsSnapshot) {
  const store = useSettingsStore.getState();
  const resource = store.settingsSnapshot || createRemoteResource<SettingsSnapshot>();
  if (!resource.data) return;
  const next = mutator(resource.data);
  store.set({
    settingsSnapshot: {
      ...resource,
      data: next,
      updatedAt: Date.now(),
    },
  });
}

export async function loadSettingsSnapshot(options: { retainSameKeyData?: boolean } = {}) {
  const owner = captureSettingsConnection();
  const store = useSettingsStore.getState();
  const myVersion = ++_settingsSnapshotLoadVersion;
  if (_settingsSnapshotAbortController) {
    _settingsSnapshotAbortController.abort();
  }
  const controller = new AbortController();
  _settingsSnapshotAbortController = controller;
  const agentId = store.getSettingsAgentId();
  const resourceKey = makeSettingsResourceKey('snapshot', agentId, settingsConnectionKey());
  const currentResource = store.settingsSnapshot || createRemoteResource<SettingsSnapshot>();
  const requestId = currentResource.requestId + 1;
  const configKey = makeSettingsResourceKey('config', agentId, settingsConnectionKey());
  const retainSameKeyData = options.retainSameKeyData === true;
  const keepSameConfigOwnerData = retainSameKeyData && store.settingsConfigKey === configKey;

  store.set({
    settingsSnapshot: startRemoteLoad(currentResource, resourceKey, requestId, { retainSameKeyData }),
    settingsConfigKey: configKey,
    settingsConfigStatus: 'loading',
    settingsConfigError: null,
    ...(keepSameConfigOwnerData ? {} : {
      settingsConfig: null,
      globalModelsConfig: null,
      homeFolder: null,
    }),
  });

  if (!agentId || !resourceKey) {
    const latest = useSettingsStore.getState();
    latest.set({
      settingsSnapshot: failRemoteLoad(latest.settingsSnapshot, resourceKey, requestId, 'No settings agent selected'),
      settingsConfigStatus: 'error',
      settingsConfigError: 'No settings agent selected',
      settingsConfig: null,
      globalModelsConfig: null,
      homeFolder: null,
    });
    return;
  }

  try {
    const res = await lingxiFetch(`/api/settings/snapshot?agentId=${encodeURIComponent(agentId)}`, {
      signal: controller.signal,
    });
    const snapshot = await res.json() as SettingsSnapshot & { error?: string };
    if (snapshot.error) throw new Error(snapshot.error);
    if (!ownsSettingsConnection(owner)) return;
    if (resourceKey !== makeSettingsResourceKey('snapshot', useSettingsStore.getState().getSettingsAgentId(), settingsConnectionKey())) return;
    if (myVersion !== _settingsSnapshotLoadVersion) return;
    if (_settingsSnapshotAbortController !== controller) return;
    applySettingsSnapshot(snapshot as SettingsSnapshot, resourceKey, requestId);
  } catch (err) {
    if (isAbortError(err)) return;
    console.error('[settings] snapshot load failed:', err);
    const latest = useSettingsStore.getState();
    if (latest.settingsSnapshot.key === resourceKey && latest.settingsSnapshot.requestId === requestId) {
      latest.set({
        settingsSnapshot: failRemoteLoad(latest.settingsSnapshot, resourceKey, requestId, err),
        settingsConfigStatus: 'error',
        settingsConfigError: err instanceof Error ? err.message : String(err),
      });
    }
  } finally {
    if (_settingsSnapshotAbortController === controller) {
      _settingsSnapshotAbortController = null;
    }
  }
}

export async function browseAgent(agentId: string) {
  useSettingsStore.setState({ settingsAgentId: agentId });
  await loadSettingsConfig();
  await loadAgents();
}

export async function switchToAgent(agentId: string, owner?: SettingsConnectionOwner | null) {
  const assertOwner = () => { if (owner && !ownsSettingsConnection(owner)) throw new Error(t('settings.agentDeletion.connectionChanged')); };
  assertOwner();
  const store = useSettingsStore.getState();
  try {
    const res = await lingxiFetch('/api/agents/switch', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id: agentId }),
    });
    const data = await res.json();
    // 走到这里响应一定是 2xx：非 2xx 已经在 hanaFetch 边界抛成带码的异常了。
    // 剩下要防的是 2xx 里仍带 error 字段的老写法，它同样要把码带过 throw。
    if (data.error) {
      const routeError = normalizeSessionRouteError(data);
      throw errorWithCode(routeError.message, routeError.code);
    }

    assertOwner();
    store.set({
      settingsAgentId: null,
      currentAgentId: data.agent.id,
      agentName: data.agent.name,
    });
    await loadSettingsConfig();
    assertOwner();
    await loadAgents();
    assertOwner();
    store.showToast(t('settings.agent.switched', { name: data.agent.name }), 'success');
  } catch (err: any) {
    console.error('[agents] switch failed:', err);
    store.showToast(t('settings.agent.switchFailed') + ': ' + localizedReasonOrRaw(err, t), 'error');
  }
}

export async function setPrimaryAgent(agentId: string) {
  const store = useSettingsStore.getState();
  try {
    const res = await lingxiFetch('/api/agents/primary', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id: agentId }),
    });
    const data = await res.json();
    // 同 switchToAgent：非 2xx 已在 hanaFetch 边界抛成带码的异常，这里只补 2xx 带
    // error 字段的老写法，同样要把码带过 throw，catch 才有得翻。
    if (data.error) {
      const routeError = normalizeSessionRouteError(data);
      throw errorWithCode(routeError.message, routeError.code);
    }

    await loadAgents();
    store.showToast(t('settings.agent.setPrimary'), 'success');
  } catch (err: any) {
    console.error('[agents] set primary failed:', err);
    store.showToast(t('settings.agent.setPrimaryFailed') + ': ' + localizedReasonOrRaw(err, t), 'error');
  }
}
