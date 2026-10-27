import fs from 'node:fs';
import path from 'node:path';
import { act } from '@testing-library/react';
import { useSettingsStore } from '../store';
import { ownsSettingsConnection } from '../connection-owner';
import { settingsConnectionKey } from '../api';
import { makeSettingsResourceKey } from '../resource-state';
import { resolveServerConnection } from '../../services/server-connection';

interface RequestReceipt {
  path: string;
  method: string;
  startedMs: number;
  finishedMs?: number;
  status?: number;
  failed?: boolean;
}

const REQUIRED_PATHS = [
  '/lingxi/v1/server/identity', '/lingxi/v1/settings/config', '/lingxi/v1/agents',
  '/lingxi/v1/settings/snapshot', '/lingxi/v1/models', '/lingxi/v1/settings/avatar-health',
];

/** 只观察现役初始化，不替换响应、不代填状态；ready 或超时兜底都不能独自放行删除。 */
export function observeAgentDeleteInitialization(options: {
  actualFetch: typeof fetch;
  base: string;
  token: string;
  home: string;
  testDeadline: number;
}) {
  const began = performance.now();
  // 沿用产品原15秒初始化边界，并受原it总60秒的剩余预算约束。
  const deadline = Math.min(options.testDeadline, began + 15_000);
  const expected = JSON.parse(fs.readFileSync(path.join(options.home, 'lingxi-service/management.json'), 'utf8'));
  const receipts: RequestReceipt[] = [];
  let identity: Record<string, unknown> | undefined;
  let identityFailure = false;
  let watchdog = false;
  let disposed = false;
  let signal = () => {};
  const previousWarn = console.warn;
  console.warn = (...args: unknown[]) => {
    if (args.some(value => typeof value === 'string' && value.includes('[settings] init timeout (15s)'))) {
      watchdog = true;
      signal();
    }
    previousWarn(...args);
  };

  const observedFetch: typeof fetch = async (input, init) => {
    const url = new URL(typeof input === 'string' || input instanceof URL ? String(input) : input.url);
    const receipt: RequestReceipt = {
      // 不保存查询参数、令牌、头或配置正文。
      path: url.pathname, method: init?.method || (input instanceof Request ? input.method : 'GET'),
      startedMs: performance.now() - began,
    };
    receipts.push(receipt);
    try {
      const response = await options.actualFetch(input, init);
      receipt.status = response.status;
      if (receipt.path === '/lingxi/v1/server/identity' && response.ok) {
        const value: unknown = await response.clone().json();
        if (value && typeof value === 'object' && !Array.isArray(value)) identity = value as Record<string, unknown>;
        else identityFailure = true;
      }
      receipt.finishedMs = performance.now() - began;
      signal();
      return response;
    } catch (failure) {
      receipt.failed = true;
      receipt.finishedMs = performance.now() - began;
      signal();
      throw failure;
    }
  };

  function completed(): boolean {
    const state = useSettingsStore.getState();
    if (watchdog) throw new Error('真实设置初始化触发15秒兜底，不能作为就绪成功');
    if (state.agentsError || state.settingsConfigError || state.runtimeModelsError
      || state.settingsConfigStatus === 'error' || state.settingsSnapshot.status === 'error') {
      throw new Error('真实设置初始化存在助手、配置或模型读取错误');
    }
    const requiredReceipts = receipts.filter(receipt => REQUIRED_PATHS.includes(receipt.path));
    if (requiredReceipts.some(receipt => receipt.failed || (receipt.status !== undefined && (receipt.status < 200 || receipt.status >= 300)))) {
      throw new Error('真实设置初始化请求失败，不能继续删除');
    }
    if (identityFailure) throw new Error('真实服务身份响应不是对象');
    if (identity && (identity.serverNodeKind !== 'lingxi-service'
      || identity.serverId !== expected.serverId || identity.serverNodeId !== expected.serverId
      || identity.studioId !== expected.studioId || identity.userId !== 'user_local'
      || identity.connectionKind !== 'local' || identity.credentialKind !== 'loopback_token'
      || identity.trustState !== 'local' || identity.authState !== 'user'
      || !Array.isArray(identity.capabilities) || !identity.capabilities.includes('studio.owner'))) {
      throw new Error('真实服务身份或本机拥有者权限与本场服务不符');
    }
    const connection = resolveServerConnection(state);
    if (connection && (connection.baseUrl !== options.base || connection.token !== options.token
      || connection.kind !== 'local' || connection.serverNodeKind !== 'lingxi-service'
      || connection.credentialKind !== 'loopback_token')) {
      throw new Error('真实初始化采用了另一连接或凭证');
    }
    if (!identity || !connection || !state.ready) return false;
    if (!REQUIRED_PATHS.every(required => requiredReceipts.some(receipt => (
      receipt.path === required && receipt.status === 200 && receipt.finishedMs !== undefined
    )))) return false;
    if (!['victim', 'survivor'].every(id => state.agents.some(agent => agent.id === id))) return false;
    if (!ownsSettingsConnection(state.settingsAgentsOwner) || !ownsSettingsConnection(state.settingsDataOwner)) return false;
    const selected = state.getSettingsAgentId();
    if (selected !== 'survivor' || state.settingsDataAgentId !== selected
      || state.settingsSnapshot.data?.agentId !== selected) return false;
    const key = settingsConnectionKey();
    return state.settingsConfigStatus === 'ready' && state.settingsSnapshot.status === 'ready'
      && state.settingsConfig !== null && state.settingsConfig.agent?.name === '保留助手'
      && state.settingsSnapshot.key === makeSettingsResourceKey('snapshot', selected, key)
      && state.settingsConfigKey === makeSettingsResourceKey('config', selected, key);
  }

  return {
    fetch: observedFetch,
    async waitForReady() {
      await act(async () => {
        await new Promise<void>((resolve, reject) => {
          let finished = false;
          let unsubscribe = () => {};
          let timer: ReturnType<typeof setTimeout> | undefined;
          const finish = (failure?: Error) => {
            if (finished) return;
            finished = true;
            unsubscribe();
            if (timer) clearTimeout(timer);
            signal = () => {};
            if (failure) reject(failure); else resolve();
          };
          signal = () => {
            if (disposed) return finish(new Error('初始化观察已提前关闭'));
            try {
              if (performance.now() >= deadline) return finish(new Error('真实初始化未在原15秒及it剩余预算内完成'));
              if (completed()) finish();
            } catch (failure) {
              finish(failure instanceof Error ? failure : new Error('真实初始化检查失败'));
            }
          };
          unsubscribe = useSettingsStore.subscribe(signal);
          timer = setTimeout(() => finish(new Error('真实初始化未在原15秒及it剩余预算内完成')), Math.max(0, deadline - performance.now()));
          signal();
        });
      });
    },
    dispose() {
      disposed = true;
      signal();
      console.warn = previousWarn;
      // 无论失败与否都留收发时间；不输出身份正文、凭证或配置。
      console.error('AGENT_DELETE_INITIALIZATION_RECEIPTS', JSON.stringify({ watchdog, identityObserved: !!identity, receipts }));
    },
  };
}
