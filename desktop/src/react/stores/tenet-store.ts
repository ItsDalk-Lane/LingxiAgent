/**
 * tenet-store — 用户原则（tenets）前端状态
 *
 * 独立小 store（settings/store.ts 同款 zustand create）：
 * - 聊天审批横幅与设置页 AgentMemory 共用；
 * - server 的 tenets-changed app_event / 本端操作后都会 refresh(agentId)。
 */

import { create } from 'zustand';
import { lingxiFetch } from '../hooks/use-hana-fetch';

export type TenetPriority = 'critical' | 'high' | 'medium' | 'low';
export type TenetStatus = 'pending' | 'active' | 'rejected';

export interface Tenet {
  id: string;
  content: string;
  priority: TenetPriority;
  status: TenetStatus;
  source: 'model_proposed' | 'user_direct';
  sessionId?: string | null;
  createdAt: string;
  decidedAt?: string | null;
}

interface TenetState {
  byAgent: Record<string, { tenets: Tenet[]; loaded: boolean }>;
  refresh: (agentId: string) => Promise<void>;
  decide: (agentId: string, tenetId: string, approve: boolean) => Promise<boolean>;
  addDirect: (agentId: string, content: string, priority?: TenetPriority) => Promise<boolean>;
  remove: (agentId: string, tenetId: string) => Promise<boolean>;
}

async function requestJson(path: string, init: RequestInit = {}): Promise<any> {
  const response = await lingxiFetch(path, {
    throwOnHttpError: false,
    ...init,
    headers: { 'content-type': 'application/json', ...(init.headers || {}) },
  });
  let data: any = null;
  try {
    data = await response.json();
  } catch {
    data = null;
  }
  if (!response.ok) {
    throw new Error(data?.error || `${response.status} ${response.statusText}`);
  }
  return data;
}

export const useTenetStore = create<TenetState>((set, get) => ({
  byAgent: {},

  refresh: async (agentId: string) => {
    if (!agentId) return;
    try {
      const data = await requestJson(`/api/agents/${encodeURIComponent(agentId)}/tenets`);
      const tenets: Tenet[] = Array.isArray(data?.tenets) ? data.tenets : [];
      set((state) => ({
        byAgent: { ...state.byAgent, [agentId]: { tenets, loaded: true } },
      }));
    } catch {
      // 拉取失败保留旧值；横幅只在 loaded 且有 pending 时出现
    }
  },

  decide: async (agentId: string, tenetId: string, approve: boolean) => {
    try {
      await requestJson(
        `/api/agents/${encodeURIComponent(agentId)}/tenets/${encodeURIComponent(tenetId)}/decide`,
        { method: 'POST', body: JSON.stringify({ approve }) },
      );
      await get().refresh(agentId);
      return true;
    } catch {
      return false;
    }
  },

  addDirect: async (agentId: string, content: string, priority?: TenetPriority) => {
    try {
      await requestJson(`/api/agents/${encodeURIComponent(agentId)}/tenets`, {
        method: 'POST',
        body: JSON.stringify({ content, ...(priority ? { priority } : {}) }),
      });
      await get().refresh(agentId);
      return true;
    } catch {
      return false;
    }
  },

  remove: async (agentId: string, tenetId: string) => {
    try {
      await requestJson(
        `/api/agents/${encodeURIComponent(agentId)}/tenets/${encodeURIComponent(tenetId)}`,
        { method: 'DELETE' },
      );
      await get().refresh(agentId);
      return true;
    } catch {
      return false;
    }
  },
}));

export function pendingTenetsOf(state: TenetState, agentId: string | null | undefined): Tenet[] {
  if (!agentId) return [];
  const entry = state.byAgent[agentId];
  return entry?.loaded ? entry.tenets.filter((t) => t.status === 'pending') : [];
}
